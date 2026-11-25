// Writes one change back into settings.toml, keeping every other byte and comment, then
// reloads (spec 0012). Also reads a raw value back for the color picker's undo (spec 0021).
use super::*;
use super::parse::{file_order, not_toml};
use super::store::{Source, path, reload};

/// A change the settings page makes to the file (spec 0012).
pub(crate) enum Edit {
    /// A key like "indicator.enabled" or "workspaces", and its new value.
    Value(&'static str, toml_edit::Value),
    /// A shortcut list, like "shortcuts.finder"; empty is off.
    Combos(&'static str, Vec<Combo>),
    /// A digit set, like "shortcuts.jump", and its modifiers; None is off.
    Digits(&'static str, Option<u8>),
    /// Every [[gesture]] block, rewritten as this list (AC-12).
    Gestures(Vec<Gesture>),
    /// Two edits in one write, so a clash is never saved halfway (AC-9).
    Pair(Box<Edit>, Box<Edit>),
    /// A value at a path of keys, like ["indicator", "screens", "<id>", "spot"]; tables on
    /// the way are made as needed, `screens` as an implicit one (spec 0020).
    At(Vec<String>, toml_edit::Value),
    /// Takes out the key or table at a path (Reset, Same as all screens). A screen table left
    /// empty goes too, and so does an empty `screens`.
    Remove(Vec<String>),
}

/// Changes one value in the file and keeps every other byte, then reloads (AC-3). The new
/// file is swapped in whole by one rename, so a crash never leaves half of it (AC-17).
pub(crate) fn write(edit: Edit) -> Result<(), String> {
    let path = path();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DEFAULT_FILE.to_string(),
        Err(e) => return Err(format!("cannot read {} ({e})", path.display())),
    };
    let new = edit_text(&text, &edit)?;
    let tmp = path.with_extension("toml.tmp");
    if let Err(e) = crate::ws::store::write_new(&tmp, &new).and_then(|()| std::fs::rename(&tmp, &path)) {
        let why = format!("cannot save {} ({e})", path.display());
        warn!("settings: {why}");
        return Err(why);
    }
    reload(Source::Page).map(|_| ())
}

/// An `[apps]` rule's key as the file spells it, for a key the settings hold lowercased: an
/// edit must name the file's own key (spec 0027 AC-14).
pub(crate) fn app_key_in_file(lower: &str) -> String {
    let text = std::fs::read_to_string(path()).unwrap_or_default();
    file_order(&text, "apps").into_iter().find(|k| k.to_lowercase() == lower).unwrap_or_else(|| lower.to_string())
}

/// The value at a path of keys as the file holds it now, None when there is no such key: the
/// color picker's undo writes it back exactly (spec 0021 AC-6).
pub(crate) fn raw_at(keys: &[String]) -> Option<toml_edit::Value> {
    let text = std::fs::read_to_string(path()).ok()?;
    raw_in(&text, keys)
}

fn raw_in(text: &str, keys: &[String]) -> Option<toml_edit::Value> {
    let doc: toml_edit::DocumentMut = text.parse().ok()?;
    let (key, tables) = keys.split_last()?;
    let table = tables.iter().try_fold(doc.as_table() as &dyn toml_edit::TableLike, |t, name| t.get(name)?.as_table_like())?;
    table.get(key)?.as_value().cloned()
}

/// For the color picker's tests: a key's value in any text, as `raw_at` reads it.
#[cfg(test)]
pub(crate) fn raw_for_test(text: &str, keys: &[String]) -> Option<toml_edit::Value> {
    raw_in(text, keys)
}

/// Pure, so the tests can run it: the file's text with the edit made and nothing else changed.
pub(super) fn edit_text(text: &str, edit: &Edit) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| not_toml(&e.to_string(), e.message()))?;
    edit_doc(&mut doc, edit)?;
    Ok(doc.to_string())
}

fn edit_doc(doc: &mut toml_edit::DocumentMut, edit: &Edit) -> Result<(), String> {
    match edit {
        Edit::Value(name, value) => set_value(doc, name, value.clone()),
        Edit::Combos(name, list) => {
            let mut items: toml_edit::Array = list.iter().map(|c| c.to_string()).collect();
            items.fmt();
            set_value(doc, name, items.into())
        }
        Edit::Digits(name, mods) => set_value(doc, name, mods.map_or(String::new(), |m| mods_text(Some(m))).into()),
        Edit::Gestures(list) => {
            set_gestures(doc, list);
            Ok(())
        }
        Edit::Pair(a, b) => edit_doc(doc, a).and_then(|()| edit_doc(doc, b)),
        Edit::At(path, value) => set_at(doc, path, value.clone()),
        Edit::Remove(path) => {
            remove_at(doc, path);
            Ok(())
        }
    }
}

fn set_at(doc: &mut toml_edit::DocumentMut, path: &[String], value: toml_edit::Value) -> Result<(), String> {
    let Some((key, tables)) = path.split_last() else { return Ok(()) };
    // Table like, so a key inside an [apps] rule's inline table is one edit too (spec 0027 AC-14).
    let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    for (i, name) in tables.iter().enumerate() {
        // A screen id is always a quoted key, whatever letters it has.
        let quoted = i > 0 && tables[i - 1] == "screens";
        let key: toml_edit::Key = if quoted { toml_edit::Value::from(name.as_str()).to_string().trim().parse().map_err(|_| format!("bad screen id {name}"))? } else { toml_edit::Key::new(name) };
        let item = table.entry_format(&key).or_insert_with(|| {
            let mut t = toml_edit::Table::new();
            // [indicator.screens."<id>"] alone, never an empty [indicator.screens] above it.
            t.set_implicit(i + 1 < tables.len());
            toml_edit::Item::Table(t)
        });
        table = item.as_table_like_mut().ok_or_else(|| format!("{}: expected a table", tables[..=i].join(".")))?;
    }
    match table.get_mut(key) {
        Some(item) => {
            let mut value = value;
            if let Some(old) = item.as_value() {
                *value.decor_mut() = old.decor().clone();
            }
            *item = toml_edit::Item::Value(value);
        }
        None => {
            table.insert(key, toml_edit::Item::Value(value));
        }
    }
    Ok(())
}

fn remove_at(doc: &mut toml_edit::DocumentMut, path: &[String]) {
    fn table_at<'a>(doc: &'a mut toml_edit::DocumentMut, path: &[String]) -> Option<&'a mut dyn toml_edit::TableLike> {
        path.iter().try_fold(doc.as_table_mut() as &mut dyn toml_edit::TableLike, |t, name| t.get_mut(name)?.as_table_like_mut())
    }
    let Some((key, tables)) = path.split_last() else { return };
    let Some(t) = table_at(doc, tables) else { return };
    t.remove(key);
    // Empty tables below a top section go: a screen table, then `screens`.
    for depth in (2..=tables.len()).rev() {
        let empty = table_at(doc, &path[..depth]).is_some_and(|t| t.is_empty());
        if !empty {
            break;
        }
        if let Some(parent) = table_at(doc, &path[..depth - 1]) {
            parent.remove(&path[depth - 1]);
        }
    }
}

/// The [[gesture]] blocks as `list`, one block per rule. The comment above the first block
/// stays; comments inside the blocks go (spec 0012 AC-12).
fn set_gestures(doc: &mut toml_edit::DocumentMut, list: &[Gesture]) {
    let first = doc.get("gesture").and_then(toml_edit::Item::as_array_of_tables).and_then(|a| a.get(0)).and_then(|t| t.decor().prefix().cloned());
    let mut blocks = toml_edit::ArrayOfTables::new();
    for (i, g) in list.iter().enumerate() {
        let mut t = toml_edit::Table::new();
        t.insert("fingers", toml_edit::value(g.fingers as i64));
        match g.motion {
            Motion::Tap => t.insert("tap", toml_edit::value(true)),
            m => t.insert("swipe", toml_edit::value(m.word())),
        };
        if g.times == 2 {
            t.insert("times", toml_edit::value(2));
        }
        t.insert("do", toml_edit::value(g.action.to_string()));
        match (i, &first) {
            (0, Some(prefix)) => t.decor_mut().set_prefix(prefix.clone()),
            _ => t.decor_mut().set_prefix("\n"),
        }
        blocks.push(t);
    }
    doc.insert("gesture", toml_edit::Item::ArrayOfTables(blocks));
}

fn set_value(doc: &mut toml_edit::DocumentMut, name: &str, value: toml_edit::Value) -> Result<(), String> {
    let (section, key) = match name.split_once('.') {
        Some((section, key)) => (Some(section), key),
        None => (None, name),
    };
    let table = match section {
        None => doc.as_table_mut(),
        Some(section) => doc.entry(section).or_insert(toml_edit::table()).as_table_mut().ok_or_else(|| format!("{section}: expected a [{section}] section"))?,
    };
    let mut value = value;
    match table.get_mut(key) {
        // get_mut, not insert, which would reformat the key. The old value's decor holds the
        // spacing and the comment after it.
        Some(item) => {
            if let Some(old) = item.as_value() {
                *value.decor_mut() = old.decor().clone();
            }
            *item = toml_edit::Item::Value(value);
        }
        None => {
            table.insert(key, toml_edit::Item::Value(value));
        }
    }
    Ok(())
}

/// For the settings page's tests: any text with an edit made.
#[cfg(test)]
pub(crate) fn apply_for_test(text: &str, edit: &Edit) -> String {
    edit_text(text, edit).unwrap()
}

/// For the settings page's tests: the default file with one value changed.
#[cfg(test)]
pub(crate) fn edit_for_test(key: &'static str, value: toml_edit::Value) -> String {
    edit_text(DEFAULT_FILE, &Edit::Value(key, value)).unwrap()
}

#[cfg(test)]
mod tests;
