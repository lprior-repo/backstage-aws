use crate::error::{Error, Result};
use regex::Regex;

pub(super) const NOTIFICATIONS_ITEM: &str = "<NotificationsSidebarItem />";
const NOTIFICATIONS_IMPORT: &str =
    "import { NotificationsSidebarItem } from '@backstage/plugin-notifications';";
const SIDEBAR_PATCH_HINT: &str = "Notifications requires this manual patch in packages/app/src/modules/nav/Sidebar.tsx:\nAfter `import { UserSettingsSignInAvatar } from '@backstage/plugin-user-settings';`, add exactly:\nimport { NotificationsSidebarItem } from '@backstage/plugin-notifications';\nBefore `{nav.take('page:user-settings')}`, add exactly:\n            <NotificationsSidebarItem />";

pub(super) fn backend_has_registration(source: &str, package: &str) -> Result<bool> {
    let package = regex::escape(package);
    let single = Regex::new(&format!(
        r"(?m)^[ \t]*backend\.add\(import\('{package}'\)\);[ \t]*\r?$"
    ))
    .map_err(|error| {
        Error::state_failed(format!(
            "cannot build backend registration matcher: {error}"
        ))
    })?;
    let multi = Regex::new(&format!(
        r"(?m)^[ \t]*backend\.add\(\r?\n[ \t]*import\('{package}'\),\r?\n[ \t]*\);[ \t]*\r?$"
    ))
    .map_err(|error| {
        Error::state_failed(format!(
            "cannot build backend registration matcher: {error}"
        ))
    })?;
    Ok(single.is_match(source) || multi.is_match(source))
}

pub(super) fn add_backend_registration(source: &str, package: &str) -> Result<String> {
    if backend_has_registration(source, package)? {
        return Ok(source.to_owned());
    }
    let Some((start, line)) = find_line(source, |line| line.contains("backend.start();")) else {
        return Err(Error::precondition_failed(
            "packages/backend/src/index.ts is missing `backend.start();`, so the plugin registration has no safe insertion point",
        ));
    };
    let indent: String = line
        .chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .collect();
    let newline = line_ending(source);
    let before = source
        .get(..start)
        .ok_or_else(|| Error::state_failed("backend insertion begins outside index.ts"))?;
    let after = source
        .get(start..)
        .ok_or_else(|| Error::state_failed("backend insertion ends outside index.ts"))?;
    Ok(format!(
        "{before}{indent}backend.add(import('{package}'));{newline}{after}"
    ))
}

pub(super) fn remove_backend_registration(source: &str, package: &str) -> Result<String> {
    let package = regex::escape(package);
    let pattern = format!(
        r"(?m)^[ \t]*backend\.add\(import\('{package}'\)\);[ \t]*(?:\r?\n|$)|^[ \t]*backend\.add\(\r?\n[ \t]*import\('{package}'\),\r?\n[ \t]*\);[ \t]*(?:\r?\n|$)"
    );
    Regex::new(&pattern)
        .map(|matcher| matcher.replace_all(source, "").into_owned())
        .map_err(|error| {
            Error::state_failed(format!("cannot build backend removal matcher: {error}"))
        })
}

pub(super) fn add_notifications_sidebar(source: &str) -> Result<String> {
    let need_import = !source.contains(NOTIFICATIONS_IMPORT);
    let need_item = !source.contains(NOTIFICATIONS_ITEM);
    let missing_import_anchor =
        need_import && !source.contains("from '@backstage/plugin-user-settings';");
    let missing_item_anchor = need_item && !source.contains("{nav.take('page:user-settings')}");
    if missing_import_anchor || missing_item_anchor {
        return Err(Error::precondition_failed(SIDEBAR_PATCH_HINT));
    }
    let with_import = if need_import {
        insert_after_line(
            source,
            "from '@backstage/plugin-user-settings';",
            NOTIFICATIONS_IMPORT,
        )?
    } else {
        source.to_owned()
    };
    if need_item {
        insert_before_line(
            &with_import,
            "{nav.take('page:user-settings')}",
            NOTIFICATIONS_ITEM,
        )
    } else {
        Ok(with_import)
    }
}

pub(super) fn notifications_sidebar_installed(source: &str) -> bool {
    source.contains(NOTIFICATIONS_IMPORT) && source.contains(NOTIFICATIONS_ITEM)
}

pub(super) fn remove_notifications_sidebar(source: &str) -> Result<String> {
    let import = Regex::new(
        r"(?m)^[ \t]*import \{ NotificationsSidebarItem \} from '@backstage/plugin-notifications';[ \t]*(?:\r?\n|$)",
    )
    .map_err(|error| Error::state_failed(format!("cannot build sidebar import matcher: {error}")))?;
    let item = Regex::new(r"(?m)^[ \t]*<NotificationsSidebarItem />[ \t]*(?:\r?\n|$)").map_err(
        |error| Error::state_failed(format!("cannot build sidebar item matcher: {error}")),
    )?;
    Ok(item
        .replace_all(&import.replace_all(source, ""), "")
        .into_owned())
}

fn insert_after_line(source: &str, marker: &str, inserted: &str) -> Result<String> {
    let Some((start, line)) = find_line(source, |line| line.contains(marker)) else {
        return Err(Error::precondition_failed(SIDEBAR_PATCH_HINT));
    };
    let end = start
        .checked_add(line.len())
        .ok_or_else(|| Error::state_failed("sidebar line offset overflow"))?;
    let indent: String = line
        .chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .collect();
    let newline = line_ending(source);
    let before = source
        .get(..end)
        .ok_or_else(|| Error::state_failed("sidebar insertion begins outside Sidebar.tsx"))?;
    let after = source
        .get(end..)
        .ok_or_else(|| Error::state_failed("sidebar insertion ends outside Sidebar.tsx"))?;
    let separator = if line.ends_with('\n') { "" } else { newline };
    Ok(format!(
        "{before}{separator}{indent}{inserted}{newline}{after}"
    ))
}

fn insert_before_line(source: &str, marker: &str, inserted: &str) -> Result<String> {
    let Some((start, line)) = find_line(source, |line| line.contains(marker)) else {
        return Err(Error::precondition_failed(SIDEBAR_PATCH_HINT));
    };
    let indent: String = line
        .chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .collect();
    let newline = line_ending(source);
    let before = source
        .get(..start)
        .ok_or_else(|| Error::state_failed("sidebar insertion begins outside Sidebar.tsx"))?;
    let after = source
        .get(start..)
        .ok_or_else(|| Error::state_failed("sidebar insertion ends outside Sidebar.tsx"))?;
    Ok(format!("{before}{indent}{inserted}{newline}{after}"))
}

fn find_line(source: &str, predicate: impl Fn(&str) -> bool) -> Option<(usize, &str)> {
    source
        .split_inclusive('\n')
        .scan(0_usize, |offset, line| {
            let start = *offset;
            *offset = offset.checked_add(line.len())?;
            Some((start, line))
        })
        .find(|(_, line)| predicate(line))
}

fn line_ending(source: &str) -> &'static str {
    if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}
