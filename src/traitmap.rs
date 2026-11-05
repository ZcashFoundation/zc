//! Trait attribution for associated public API items.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::rc::Rc;

use serde_json::Value;

use crate::ctx::Ctx;
use crate::git;
use crate::rustdoc::PublicItems;

/// A missing attribution means an inherent member or conflicting trait implementations.
pub type TraitMap = HashMap<(String, String), Option<String>>;

/// Builds per-crate trait maps at one ref.
pub fn dump(ctx: &Ctx, ref_sha: &str, crates: &[String]) -> HashMap<String, TraitMap> {
    let Ok(sha) = git::rev_parse_verify(ref_sha) else {
        return HashMap::new();
    };
    let target = ctx.tmp.dir.join("trait-target");
    let _ = fs::create_dir_all(&target);
    let worktree = match ctx.tmp.sub("trait") {
        Ok(path) => path,
        Err(_) => return HashMap::new(),
    };
    if git::worktree_add(&worktree, ref_sha).is_err() {
        return HashMap::new();
    }

    let mut maps = HashMap::new();
    for (index, crate_name) in crates.iter().enumerate() {
        let cache_name = format!(
            "{}.{}.{}.{}.traitmap-v2.tsv",
            sha, ctx.cache.script_hash, ctx.cache.api_fp, crate_name
        );
        let cache_path = ctx.cache.path(&cache_name);
        if let Ok(cached) = fs::read_to_string(&cache_path) {
            maps.insert(crate_name.clone(), parse_tsv(&cached));
            continue;
        }

        ctx.progress.set(&format!(
            "--changelog: trait map [{}/{}] {crate_name}",
            index + 1,
            crates.len()
        ));
        if let Ok(path) = crate::api::rustdoc_json(ctx, crate_name, &worktree, &target, &sha) {
            let rows = fs::read_to_string(path)
                .ok()
                .map(|json| extract(&json))
                .unwrap_or_default();
            let tsv = render_tsv(&rows);
            ctx.cache.write_atomic(&cache_name, &tsv);
            maps.insert(crate_name.clone(), rows_to_map(&rows));
        } else {
            maps.insert(crate_name.clone(), TraitMap::new());
        }
    }
    git::worktree_remove(&worktree);
    maps
}

fn id_key(id: &Value) -> String {
    id.as_str().map_or_else(|| id.to_string(), str::to_string)
}

/// Renders a rustdoc path with its complete generic arguments.
pub(crate) fn path_name(root: &Value, path: &Value) -> Option<String> {
    Names {
        root,
        public: None,
        owner: None,
        comparison: None,
    }
    .path(path)
}

fn public_item_paths<'a>(
    root: &'a Value,
    path: &Value,
    public: &'a PublicItems<'a>,
) -> impl Iterator<Item = &'a str> {
    let item = path
        .get("id")
        .and_then(|id| root.get("index")?.get(id_key(id)));
    public.items.iter().filter_map(move |(path, public_item)| {
        item.filter(|item| std::ptr::eq(*public_item, *item))
            .map(|_| path.as_str())
    })
}

/// Renders a path using cargo-public-api's preferred public export.
pub(crate) fn public_path_name(
    root: &Value,
    path: &Value,
    public: &PublicItems<'_>,
) -> Option<String> {
    if public.items.is_empty() {
        return path_name(root, path);
    }
    Names {
        root,
        public: Some(public),
        owner: None,
        comparison: None,
    }
    .path(path)
}

/// Keeps every exported trait identity so adding an alias cannot hide existing values.
pub(crate) fn public_path_names(
    root: &Value,
    path: &Value,
    public: &PublicItems<'_>,
) -> Option<Vec<String>> {
    let exported: Vec<_> = public_item_paths(root, path, public).collect();
    if exported.is_empty() {
        return Some(vec![public_path_name(root, path, public)?]);
    }
    let names = Names {
        root,
        public: Some(public),
        owner: None,
        comparison: None,
    };
    let args = path
        .get("args")
        .filter(|args| !args.is_null())
        .map_or(Some(String::new()), |args| names.args(args))?;
    let paths = exported
        .into_iter()
        .map(|name| format!("{name}{args}"))
        .collect();
    Some(paths)
}

pub(crate) fn public_type_name(
    root: &Value,
    ty: &Value,
    public: &PublicItems<'_>,
) -> Option<String> {
    if public.items.is_empty() {
        return type_name(root, ty);
    }
    Names {
        root,
        public: Some(public),
        owner: None,
        comparison: None,
    }
    .ty(ty)
}

/// Renders an impl owner through one particular export, without rewriting literals.
pub(crate) fn public_owner_name(
    root: &Value,
    ty: &Value,
    public: &PublicItems<'_>,
    owner_id: &Value,
    owner_path: &str,
) -> Option<String> {
    Names {
        root,
        public: Some(public),
        owner: Some((owner_id, owner_path)),
        comparison: None,
    }
    .ty(ty)
}

/// Exact rendered impl structure plus the surviving public slots of each named item.
/// Reusing a slot number preserves repeated-item constraints without expanding alias combinations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImplIdentity {
    pub(crate) shape: String,
    pub(crate) origin: String,
    slots: Rc<Vec<Vec<String>>>,
}

impl ImplIdentity {
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.shape == other.shape
            && self.slots.len() == other.slots.len()
            && self
                .slots
                .iter()
                .zip(other.slots.iter())
                .all(|(old, new)| old.iter().any(|path| new.binary_search(path).is_ok()))
    }

    pub(crate) fn has_alternative_slots(&self) -> bool {
        self.slots.iter().any(|paths| paths.len() > 1)
    }

    pub(crate) fn shared_path(&self, other: &Self) -> Option<String> {
        if self.shape != other.shape || self.slots.len() != other.slots.len() {
            return None;
        }
        let mut parts = self.shape.split('\0');
        let mut path = String::with_capacity(self.shape.len());
        path.push_str(parts.next()?);
        while let Some(slot) = parts.next() {
            let slot: usize = slot.parse().ok()?;
            let before = self.slots.get(slot)?;
            let after = other.slots.get(slot)?;
            let shared = before
                .iter()
                .find(|name| after.binary_search(name).is_ok())?;
            path.push_str(shared);
            path.push_str(parts.next()?);
        }
        Some(path)
    }

    pub(crate) fn member(&self, name: &str) -> Self {
        Self {
            shape: format!("{}::{name}", self.shape),
            origin: format!("{}::{name}", self.origin),
            slots: self.slots.clone(),
        }
    }
}

#[derive(Default)]
struct ComparisonNames {
    ids: Vec<String>,
    slots: Vec<Vec<String>>,
}

impl ComparisonNames {
    fn name(&mut self, root: &Value, path: &Value, public: &PublicItems<'_>) -> Option<String> {
        let item = crate::rustdoc::item(root, path.get("id")?)?;
        let producer = crate::rustdoc::item(root, root.get("root")?)?;
        if item.get("crate_id") != producer.get("crate_id") {
            return None;
        }
        let id = crate::rustdoc::id_text(path.get("id")?)?;
        if let Some(slot) = self.ids.iter().position(|seen| seen == &id) {
            return Some(format!("\0{slot}\0"));
        }
        let paths: Vec<_> = public_item_paths(root, path, public)
            .map(str::to_string)
            .collect();
        if paths.is_empty() {
            return None;
        }
        let slot = self.ids.len();
        self.ids.push(id);
        self.slots.push(paths);
        Some(format!("\0{slot}\0"))
    }
}

pub(crate) fn impl_identity(
    root: &Value,
    implementation: &Value,
    public: &PublicItems<'_>,
) -> Option<ImplIdentity> {
    let owner_type = implementation.get("for")?;
    let trait_path = implementation.get("trait").filter(|v| !v.is_null());
    let canonical_owner = type_name(root, owner_type)?;
    let origin = if let Some(trait_path) = trait_path {
        format!("<{canonical_owner} as {}>", path_name(root, trait_path)?)
    } else {
        canonical_owner
    };
    let comparison = RefCell::new(ComparisonNames::default());
    let names = Names {
        root,
        public: Some(public),
        owner: None,
        comparison: Some(&comparison),
    };
    let owner = names.ty(owner_type)?;
    let shape = if let Some(trait_path) = trait_path {
        format!("<{owner} as {}>", names.path(trait_path)?)
    } else {
        owner
    };
    Some(ImplIdentity {
        shape,
        origin,
        slots: Rc::new(comparison.into_inner().slots),
    })
}

struct Names<'a> {
    root: &'a Value,
    public: Option<&'a PublicItems<'a>>,
    owner: Option<(&'a Value, &'a str)>,
    comparison: Option<&'a RefCell<ComparisonNames>>,
}

impl Names<'_> {
    fn path(&self, path: &Value) -> Option<String> {
        let canonical = path
            .get("id")
            .and_then(|id| self.root.get("paths")?.get(id_key(id)))
            .and_then(|item| item.get("path"))
            .and_then(Value::as_array)
            .and_then(|segments| {
                segments
                    .iter()
                    .map(Value::as_str)
                    .collect::<Option<Vec<_>>>()
            })
            .map(|segments| segments.join("::"))
            .or_else(|| path.get("path")?.as_str().map(str::to_string))?;
        let comparison_name = self
            .comparison
            .and_then(|comparison| comparison.borrow_mut().name(self.root, path, self.public?));
        let name = if let Some(name) = comparison_name.as_deref() {
            name
        } else if self.comparison.is_some() {
            &canonical
        } else if let Some((_, name)) = self.owner.filter(|(id, _)| path.get("id") == Some(*id)) {
            name
        } else if let Some(public) = self.public {
            path.get("id")
                .and_then(|id| public.preferred_path(id))
                .unwrap_or(&canonical)
        } else {
            &canonical
        };
        let args = path.get("args").filter(|args| !args.is_null());
        Some(format!(
            "{name}{}",
            args.map_or(Some(String::new()), |args| self.args(args))?
        ))
    }

    fn args(&self, args: &Value) -> Option<String> {
        if let Some(args) = args.get("angle_bracketed") {
            let mut rendered = Vec::new();
            for arg in args.get("args")?.as_array()? {
                rendered.push(if let Some(ty) = arg.get("type") {
                    self.ty(ty)?
                } else if let Some(lifetime) = arg.get("lifetime").and_then(Value::as_str) {
                    lifetime.to_string()
                } else if let Some(value) = arg.get("const") {
                    value.get("expr")?.as_str()?.to_string()
                } else if arg.as_str() == Some("infer") {
                    "_".to_string()
                } else {
                    return None;
                });
            }
            for constraint in args
                .get("constraints")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = constraint.get("name")?.as_str()?;
                let suffix = constraint
                    .get("args")
                    .filter(|args| !args.is_null())
                    .map_or(Some(String::new()), |args| self.args(args))?;
                let binding = constraint.get("binding")?;
                if let Some(binding) = binding.get("equality") {
                    let value = if let Some(ty) = binding.get("type") {
                        self.ty(ty)?
                    } else {
                        binding.get("constant")?.get("expr")?.as_str()?.to_string()
                    };
                    rendered.push(format!("{name}{suffix} = {value}"));
                } else {
                    let bounds = binding
                        .get("constraint")?
                        .as_array()?
                        .iter()
                        .map(|bound| self.bound(bound))
                        .collect::<Option<Vec<_>>>()?;
                    rendered.push(format!("{name}{suffix}: {}", bounds.join(" + ")));
                }
            }
            return Some(if rendered.is_empty() {
                String::new()
            } else {
                format!("<{}>", rendered.join(", "))
            });
        }
        if let Some(args) = args.get("parenthesized") {
            let inputs = args
                .get("inputs")?
                .as_array()?
                .iter()
                .map(|ty| self.ty(ty))
                .collect::<Option<Vec<_>>>()?;
            let output = args
                .get("output")
                .filter(|ty| !ty.is_null())
                .map_or(Some(String::new()), |ty| {
                    Some(format!(" -> {}", self.ty(ty)?))
                })?;
            return Some(format!("({}){output}", inputs.join(", ")));
        }
        (args.as_str() == Some("return_type_notation")).then(|| "(..)".to_string())
    }
}

fn binder(params: Option<&Value>) -> Option<String> {
    let mut names = Vec::new();
    for param in params.and_then(Value::as_array).into_iter().flatten() {
        let lifetime = param.pointer("/kind/lifetime")?;
        let mut name = param.get("name")?.as_str()?.to_string();
        if let Some(bounds) = lifetime.get("outlives").and_then(Value::as_array) {
            if !bounds.is_empty() {
                let bounds = bounds
                    .iter()
                    .map(Value::as_str)
                    .collect::<Option<Vec<_>>>()?;
                name.push_str(&format!(": {}", bounds.join(" + ")));
            }
        }
        names.push(name);
    }
    Some(if names.is_empty() {
        String::new()
    } else {
        format!("for<{}> ", names.join(", "))
    })
}

/// Renders canonical types without recursively collecting public items.
pub(crate) fn type_name(root: &Value, ty: &Value) -> Option<String> {
    Names {
        root,
        public: None,
        owner: None,
        comparison: None,
    }
    .ty(ty)
}

impl Names<'_> {
    fn bound(&self, bound: &Value) -> Option<String> {
        if let Some(lifetime) = bound.get("outlives").and_then(Value::as_str) {
            return Some(lifetime.to_string());
        }
        if let Some(args) = bound.get("use").and_then(Value::as_array) {
            let names = args
                .iter()
                .map(|arg| arg.get("lifetime").or_else(|| arg.get("param"))?.as_str())
                .collect::<Option<Vec<_>>>()?;
            return Some(format!("use<{}>", names.join(", ")));
        }
        let bound = bound.get("trait_bound")?;
        let modifier = match bound
            .get("modifier")
            .and_then(Value::as_str)
            .unwrap_or("none")
        {
            "none" => "",
            "maybe" => "?",
            "maybe_const" => "~const ",
            _ => return None,
        };
        Some(format!(
            "{}{modifier}{}",
            binder(bound.get("generic_params"))?,
            self.path(bound.get("trait")?)?
        ))
    }

    fn ty(&self, ty: &Value) -> Option<String> {
        if let Some(path) = ty.get("resolved_path") {
            return self.path(path);
        }
        if let Some(name) = ty
            .get("generic")
            .or_else(|| ty.get("primitive"))
            .and_then(Value::as_str)
        {
            return Some(name.to_string());
        }
        if let Some(reference) = ty.get("borrowed_ref") {
            let lifetime = reference
                .get("lifetime")
                .and_then(Value::as_str)
                .map_or(String::new(), |name| format!("{name} "));
            let mutable = if reference.get("is_mutable")?.as_bool()? {
                "mut "
            } else {
                ""
            };
            return Some(format!(
                "&{lifetime}{mutable}{}",
                self.ty(reference.get("type")?)?
            ));
        }
        if let Some(pointer) = ty.get("raw_pointer") {
            let mutable = if pointer.get("is_mutable")?.as_bool()? {
                "mut"
            } else {
                "const"
            };
            return Some(format!("*{mutable} {}", self.ty(pointer.get("type")?)?));
        }
        if let Some(slice) = ty.get("slice") {
            return Some(format!("[{}]", self.ty(slice)?));
        }
        if let Some(array) = ty.get("array") {
            return Some(format!(
                "[{}; {}]",
                self.ty(array.get("type")?)?,
                array.get("len")?.as_str()?
            ));
        }
        if let Some(tuple) = ty.get("tuple").and_then(Value::as_array) {
            let values = tuple
                .iter()
                .map(|ty| self.ty(ty))
                .collect::<Option<Vec<_>>>()?;
            let suffix = if values.len() == 1 { "," } else { "" };
            return Some(format!("({}{suffix})", values.join(", ")));
        }
        if ty.as_str() == Some("infer") {
            return Some("_".to_string());
        }
        if let Some(path) = ty.get("qualified_path") {
            let owner = self.ty(path.get("self_type")?)?;
            let owner = if let Some(trait_path) = path.get("trait").filter(|value| !value.is_null())
            {
                format!("<{owner} as {}>", self.path(trait_path)?)
            } else {
                owner
            };
            let args = path
                .get("args")
                .filter(|value| !value.is_null())
                .map_or(Some(String::new()), |args| self.args(args))?;
            return Some(format!("{owner}::{}{args}", path.get("name")?.as_str()?));
        }
        if let Some(dynamic) = ty.get("dyn_trait") {
            let mut bounds = Vec::new();
            for bound in dynamic.get("traits")?.as_array()? {
                bounds.push(format!(
                    "{}{}",
                    binder(bound.get("generic_params"))?,
                    self.path(bound.get("trait")?)?
                ));
            }
            if let Some(lifetime) = dynamic.get("lifetime").and_then(Value::as_str) {
                bounds.push(lifetime.to_string());
            }
            return Some(format!("dyn {}", bounds.join(" + ")));
        }
        if let Some(bounds) = ty.get("impl_trait").and_then(Value::as_array) {
            let bounds = bounds
                .iter()
                .map(|bound| self.bound(bound))
                .collect::<Option<Vec<_>>>()?;
            return Some(format!("impl {}", bounds.join(" + ")));
        }
        if let Some(function) = ty.get("function_pointer") {
            let signature = function.get("sig")?;
            let mut inputs = signature
                .get("inputs")?
                .as_array()?
                .iter()
                .map(|input| self.ty(input.get(1)?))
                .collect::<Option<Vec<_>>>()?;
            if signature.get("is_c_variadic").and_then(Value::as_bool) == Some(true) {
                inputs.push("...".to_string());
            }
            let output = signature
                .get("output")
                .filter(|value| !value.is_null())
                .map_or(Some(String::new()), |ty| {
                    Some(format!(" -> {}", self.ty(ty)?))
                })?;
            let header = function.get("header")?;
            let unsafe_ = if header.get("is_unsafe").and_then(Value::as_bool) == Some(true) {
                "unsafe "
            } else {
                ""
            };
            let abi = header.get("abi")?;
            let abi = if abi.as_str() == Some("Rust") {
                String::new()
            } else if let Some(other) = abi.get("Other").and_then(Value::as_str) {
                format!("extern {other:?} ")
            } else {
                let (name, flags) = abi.as_object()?.iter().next()?;
                let name = if name == "C" {
                    name.clone()
                } else {
                    name.to_lowercase()
                };
                let unwind = if flags.get("unwind").and_then(Value::as_bool) == Some(true) {
                    "-unwind"
                } else {
                    ""
                };
                format!("extern \"{name}{unwind}\" ")
            };
            return Some(format!(
                "{}{unsafe_}{abi}fn({}){output}",
                binder(function.get("generic_params"))?,
                inputs.join(", ")
            ));
        }
        None
    }
}

fn extract(json: &str) -> Vec<(String, String, String)> {
    let Ok(root) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(index) = root.get("index").and_then(Value::as_object) else {
        return Vec::new();
    };
    let public = crate::rustdoc::public_items(&root);
    let mut rows = Vec::new();
    for item in index.values() {
        let Some(implementation) = item.get("inner").and_then(|inner| inner.get("impl")) else {
            continue;
        };
        let Some(owner) = implementation
            .get("for")
            .and_then(|ty| public_type_name(&root, ty, &public))
        else {
            continue;
        };
        let trait_path = match implementation.get("trait").filter(|item| !item.is_null()) {
            Some(path) => public_path_name(&root, path, &public),
            None => Some(String::new()),
        };
        // Unknown generic forms must block attribution, not silently claim a different impl.
        let trait_path = trait_path.unwrap_or_default();
        let mut owners = vec![owner.clone()];
        if let Some(path) = implementation
            .get("for")
            .and_then(|ty| ty.get("resolved_path"))
        {
            for exported in public_item_paths(&root, path, &public) {
                let Some(alias) = public_owner_name(
                    &root,
                    &implementation["for"],
                    &public,
                    &path["id"],
                    exported,
                ) else {
                    continue;
                };
                if !owners.contains(&alias) {
                    owners.push(alias);
                }
            }
        }
        for id in implementation
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(member) = index
                .get(&id_key(id))
                .and_then(|item| item.get("name"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            for owner in &owners {
                rows.push((owner.clone(), member.to_string(), trait_path.clone()));
            }
        }
    }
    rows.sort_unstable();
    rows.dedup();
    rows
}

fn rows_to_map(rows: &[(String, String, String)]) -> TraitMap {
    let mut map = TraitMap::new();
    for (owner, member, trait_path) in rows {
        let attribution = (!trait_path.is_empty()).then(|| trait_path.clone());
        map.entry((owner.clone(), member.clone()))
            .and_modify(|existing| {
                if *existing != attribution {
                    *existing = None;
                }
            })
            .or_insert(attribution);
    }
    map
}

fn render_tsv(rows: &[(String, String, String)]) -> String {
    let mut output = String::new();
    for (self_name, member, trait_path) in rows {
        output.push_str(self_name);
        output.push('\t');
        output.push_str(member);
        output.push('\t');
        output.push_str(trait_path);
        output.push('\n');
    }
    output
}

fn parse_tsv(contents: &str) -> TraitMap {
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in contents.lines() {
        let mut fields = line.split('\t');
        let (Some(self_name), Some(member), Some(trait_path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let row = (
            self_name.to_string(),
            member.to_string(),
            trait_path.to_string(),
        );
        if seen.insert(row.clone()) {
            rows.push(row);
        }
    }
    rows_to_map(&rows)
}

#[cfg(test)]
#[path = "traitmap/tests.rs"]
mod tests;
