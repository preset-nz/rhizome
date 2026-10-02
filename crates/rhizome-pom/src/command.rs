//! Commands, per the commands-first contract of `plugin-primitive.md`: an id, a label, an
//! `enabled` test, and a `run`. POM supplies the ones every app needs; an app adds its own.
//! `native-menu` builds the menu from them; POM never touches a menu.

use rhizome_core::{Commit, Fragment, Op};
use serde::de::DeserializeOwned;
use serde_json::Value as Json;

use crate::document::Document;
use crate::error::{Error, Result};
use crate::model::ObjectModel;
use crate::presets::PresetRef;

/// What a command did.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Nothing,
    Committed(Commit),
    /// Text for the caller: a copied fragment for the clipboard, an exported preset file.
    Text(String),
}

impl From<Option<Commit>> for Outcome {
    fn from(c: Option<Commit>) -> Self {
        c.map_or(Outcome::Nothing, Outcome::Committed)
    }
}

type Label<M> = Box<dyn Fn(&Document<M>) -> String + Send + Sync>;
type Enabled<M> = Box<dyn Fn(&Document<M>, &Json) -> bool + Send + Sync>;
type Run<M> = Box<dyn Fn(&mut Document<M>, &Json) -> Result<Outcome> + Send + Sync>;

pub struct Command<M: ObjectModel> {
    pub id: String,
    pub label: Label<M>,
    pub enabled: Enabled<M>,
    pub run: Run<M>,
}

pub struct Commands<M: ObjectModel> {
    list: Vec<Command<M>>,
}

/// Reads a command's JSON payload into the shape it wants.
pub fn payload<T: DeserializeOwned>(p: &Json) -> Result<T> {
    serde_json::from_value(p.clone()).map_err(|e| Error::Payload(e.to_string()))
}

#[derive(serde::Deserialize)]
struct At {
    at: String,
}

#[derive(serde::Deserialize)]
struct Paste {
    parent: String,
    fragment: String,
}

#[derive(serde::Deserialize)]
struct PresetArgs {
    at: String,
    #[serde(default)]
    preset: Option<PresetRef>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

#[derive(serde::Deserialize)]
struct AddArgs {
    parent: String,
    #[serde(rename = "type")]
    type_name: String,
    name: String,
    #[serde(default)]
    preset: Option<PresetRef>,
}

#[derive(serde::Deserialize)]
struct ExportArgs {
    at: String,
    labels: Vec<String>,
}

#[derive(serde::Deserialize)]
struct ImportArgs {
    text: String,
}

fn node_of<M: ObjectModel>(d: &Document<M>, at: &str) -> Result<rhizome_core::NodeId> {
    d.tree()
        .at(at)
        .map(|n| n.id())
        .ok_or_else(|| Error::Rhizome(rhizome_core::Error::NotFound(at.into())))
}

fn need<T>(v: Option<T>, what: &str) -> Result<T> {
    v.ok_or_else(|| Error::Payload(format!("missing `{what}`")))
}

impl<M: ObjectModel> Commands<M> {
    /// Adds a command. A later one with the same id replaces an earlier one, so an app can
    /// override a built-in.
    pub fn add(
        &mut self,
        id: &str,
        label: impl Fn(&Document<M>) -> String + Send + Sync + 'static,
        enabled: impl Fn(&Document<M>, &Json) -> bool + Send + Sync + 'static,
        run: impl Fn(&mut Document<M>, &Json) -> Result<Outcome> + Send + Sync + 'static,
    ) -> &mut Self {
        self.list.retain(|c| c.id != id);
        self.list.push(Command {
            id: id.to_string(),
            label: Box::new(label),
            enabled: Box::new(enabled),
            run: Box::new(run),
        });
        self
    }

    pub(crate) fn get(&self, id: &str) -> Result<&Command<M>> {
        self.list
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| Error::UnknownCommand(id.into()))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Command<M>> {
        self.list.iter()
    }

    pub(crate) fn builtin() -> Self {
        let mut c = Commands { list: Vec::new() };
        let fixed = |s: &'static str| move |_: &Document<M>| s.to_string();
        let always = |_: &Document<M>, _: &Json| true;

        c.add("file.save", fixed("Save"), always, |d, _| {
            d.save()?;
            Ok(Outcome::Nothing)
        });
        c.add(
            "file.revert",
            fixed("Revert to Saved"),
            |d, _| d.path().is_some() && d.is_unsaved(),
            |d, _| {
                d.revert()?;
                Ok(Outcome::Nothing)
            },
        );
        c.add(
            "edit.undo",
            |d| {
                d.tree()
                    .undo_label()
                    .map_or("Undo".into(), |l| format!("Undo {l}"))
            },
            |d, _| d.tree().undo_label().is_some() && !d.tree().gesture_open(),
            |d, _| Ok(d.undo()?.into()),
        );
        c.add(
            "edit.redo",
            |d| {
                d.tree()
                    .redo_label()
                    .map_or("Redo".into(), |l| format!("Redo {l}"))
            },
            |d, _| d.tree().redo_label().is_some() && !d.tree().gesture_open(),
            |d, _| Ok(d.redo()?.into()),
        );
        c.add(
            "edit.delete",
            fixed("Delete"),
            |d, p| {
                let Ok(At { at }) = payload(p) else {
                    return false;
                };
                let Some(n) = d.tree().at(at.as_str()) else {
                    return false;
                };
                !n.is_root()
                    && !n.is_category()
                    && !n.is_opaque()
                    && d.policy(n.type_name()).deletable
            },
            |d, p| {
                let At { at } = payload(p)?;
                Ok(d.edit_ops("Delete", &[Op::Remove { at }])?.into())
            },
        );
        c.add(
            "edit.duplicate",
            fixed("Duplicate"),
            |d, p| {
                let Ok(At { at }) = payload(p) else {
                    return false;
                };
                let Some(n) = d.tree().at(at.as_str()) else {
                    return false;
                };
                if n.is_root() || n.is_category() || n.is_opaque() {
                    return false;
                }
                let policy = d.policy(n.type_name());
                let siblings = n.parent().map_or(0, |p| {
                    p.children()
                        .filter(|c| c.type_name() == n.type_name())
                        .count()
                });
                policy.duplicable && policy.max_per_parent.is_none_or(|max| siblings < max)
            },
            |d, p| {
                let At { at } = payload(p)?;
                Ok(d.duplicate(&at)?.into())
            },
        );
        c.add(
            "edit.copy",
            fixed("Copy"),
            |d, p| {
                let Ok(At { at }) = payload(p) else {
                    return false;
                };
                d.tree()
                    .at(at.as_str())
                    .is_some_and(|n| !n.is_root() && !n.is_category())
            },
            |d, p| {
                let At { at } = payload(p)?;
                Ok(Outcome::Text(d.tree().extract([at])?.to_text()))
            },
        );
        c.add(
            "edit.paste",
            fixed("Paste"),
            |d, p| {
                let Ok(Paste { parent, fragment }) = payload(p) else {
                    return false;
                };
                d.tree().at(parent.as_str()).is_some() && Fragment::from_text(&fragment).is_ok()
            },
            |d, p| {
                let Paste { parent, fragment } = payload(p)?;
                Ok(d.paste(&parent, &fragment)?.into())
            },
        );

        c.add(
            "node.add",
            fixed("New"),
            |d, p| payload::<AddArgs>(p).is_ok_and(|a| d.tree().at(a.parent.as_str()).is_some()),
            |d, p| {
                let a: AddArgs = payload(p)?;
                Ok(match a.preset {
                    Some(preset) => d
                        .add_from_preset(&a.parent, &a.type_name, &a.name, &preset)?
                        .1
                        .into(),
                    None => d
                        .edit_ops(
                            &format!("New {}", a.type_name),
                            &[Op::Add {
                                parent: a.parent,
                                type_name: a.type_name,
                                name: a.name,
                            }],
                        )?
                        .into(),
                })
            },
        );

        let preset_enabled = |d: &Document<M>, p: &Json| {
            let Ok(a) = payload::<PresetArgs>(p) else {
                return false;
            };
            d.tree()
                .at(a.at.as_str())
                .is_some_and(|n| d.has_presets(n.type_name()))
        };
        c.add(
            "preset.apply",
            fixed("Apply Preset"),
            preset_enabled,
            |d, p| {
                let a: PresetArgs = payload(p)?;
                let node = node_of(d, &a.at)?;
                Ok(d.apply_preset(node, &need(a.preset, "preset")?)?.1.into())
            },
        );
        c.add(
            "preset.save",
            fixed("Save Preset…"),
            preset_enabled,
            |d, p| {
                let a: PresetArgs = payload(p)?;
                let node = node_of(d, &a.at)?;
                Ok(d.save_preset(node, &need(a.label, "label")?)?.into())
            },
        );
        c.add(
            "preset.update",
            fixed("Update Preset"),
            preset_enabled,
            |d, p| {
                let a: PresetArgs = payload(p)?;
                let node = node_of(d, &a.at)?;
                Ok(d.update_preset(node, &need(a.label, "label")?)?.into())
            },
        );
        c.add(
            "preset.rename",
            fixed("Rename Preset…"),
            preset_enabled,
            |d, p| {
                let a: PresetArgs = payload(p)?;
                let node = node_of(d, &a.at)?;
                Ok(
                    d.rename_preset(node, &need(a.label, "label")?, &need(a.to, "to")?)?
                        .into(),
                )
            },
        );
        c.add(
            "preset.delete",
            fixed("Delete Preset"),
            preset_enabled,
            |d, p| {
                let a: PresetArgs = payload(p)?;
                let node = node_of(d, &a.at)?;
                Ok(d.delete_preset(node, &need(a.label, "label")?)?.into())
            },
        );
        c.add(
            "preset.export",
            fixed("Export Presets…"),
            preset_enabled,
            |d, p| {
                let a: ExportArgs = payload(p)?;
                let node = node_of(d, &a.at)?;
                Ok(Outcome::Text(d.export_presets(node, &a.labels)?))
            },
        );
        c.add(
            "preset.import",
            fixed("Import Presets…"),
            |_, p| {
                payload::<ImportArgs>(p).is_ok_and(|a| crate::presets::read_file(&a.text).is_ok())
            },
            |d, p| {
                let a: ImportArgs = payload(p)?;
                Ok(d.import_presets(&a.text)?.1.into())
            },
        );
        c
    }
}
