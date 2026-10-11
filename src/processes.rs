//! Display groups retain every individual PID/start-time identity for actions.
use crate::monitor::ProcessRow;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    uid: Option<u32>,
    executable: Option<String>,
    label: String,
    application: bool,
    unverified: Option<(u32, u64)>,
}
pub fn key(row: &ProcessRow) -> Key {
    Key {
        uid: row.uid,
        executable: row.executable.clone(),
        label: row.application.clone().unwrap_or_else(|| row.name.clone()),
        application: row.application.is_some(),
        unverified: (row.executable.is_none() || row.uid.is_none())
            .then_some((row.pid, row.started)),
    }
}
pub struct Group {
    pub key: Key,
    pub indices: Vec<usize>,
    pub summary: ProcessRow,
}
pub fn groups(visible: &[usize], rows: &[ProcessRow]) -> Vec<Group> {
    let mut positions = HashMap::new();
    let mut seen = HashSet::new();
    let mut result: Vec<Group> = Vec::new();
    for &index in visible {
        let row = &rows[index];
        if !seen.insert((row.pid, row.started)) {
            continue;
        }
        let key = key(row);
        if let Some(&position) = positions.get(&key) {
            let group: &mut Group = &mut result[position];
            group.indices.push(index);
            group.summary.cpu += row.cpu;
            group.summary.memory = group.summary.memory.saturating_add(row.memory);
            group.summary.threads = group.summary.threads.saturating_add(row.threads);
            group.summary.io_rate = group.summary.io_rate.zip(row.io_rate).map(|(a, b)| a + b);
            if group.summary.status != row.status {
                group.summary.status = "Verschieden".into();
            }
        } else {
            positions.insert(key.clone(), result.len());
            result.push(Group {
                key,
                indices: vec![index],
                summary: row.clone(),
            });
        }
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Heading(bool, usize),
    Group(usize),
    Process(usize, bool),
}
pub fn entries(
    groups: &[Group],
    opened: &HashSet<Key>,
    applications_open: bool,
    background_open: bool,
    category: Option<bool>,
) -> Vec<Entry> {
    let mut entries = Vec::new();
    for applications in [true, false] {
        if category.is_some_and(|value| value != applications) {
            continue;
        }
        let count = groups
            .iter()
            .filter(|g| g.summary.application.is_some() == applications)
            .map(|g| g.indices.len())
            .sum();
        entries.push(Entry::Heading(applications, count));
        if !(if applications {
            applications_open
        } else {
            background_open
        }) {
            continue;
        }
        for (i, group) in groups
            .iter()
            .enumerate()
            .filter(|(_, g)| g.summary.application.is_some() == applications)
        {
            if group.indices.len() == 1 {
                entries.push(Entry::Process(group.indices[0], false));
            } else {
                entries.push(Entry::Group(i));
                if opened.contains(&group.key) {
                    entries.extend(group.indices.iter().map(|i| Entry::Process(*i, true)));
                }
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(pid: u32) -> ProcessRow {
        ProcessRow {
            pid,
            started: 100 + pid as u64,
            parent_pid: 10,
            cgroup: None,
            name: "worker".into(),
            status: "Bereit".into(),
            cpu: 2.0,
            memory: 100,
            io_rate: Some(20.0),
            uid: Some(1000),
            user: "test".into(),
            threads: 3,
            command: "worker".into(),
            executable: Some("/usr/bin/worker".into()),
            application: Some("Worker".into()),
        }
    }
    #[test]
    fn groups_aggregate_visible_identities_and_keep_different_executables_owners_or_apps_separate()
    {
        let mut other_exe = row(3);
        other_exe.executable = Some("/opt/worker".into());
        let mut other_user = row(4);
        other_user.uid = Some(1001);
        let mut other_app = row(5);
        other_app.application = Some("Other".into());
        let mut unverified = row(6);
        unverified.executable = None;
        let mut missing_io = row(2);
        missing_io.io_rate = None;
        missing_io.status = "Wird ausgeführt".into();
        let rows = vec![
            row(1),
            missing_io,
            other_exe,
            other_user,
            other_app,
            unverified,
        ];
        let groups = groups(&[1, 0, 1, 2, 3, 4, 5], &rows);
        assert_eq!(groups.len(), 5);
        assert_eq!(groups[0].indices, vec![1, 0]);
        assert_eq!(groups[0].summary.memory, 200);
        assert_eq!(groups[0].summary.cpu, 4.0);
        assert_eq!(groups[0].summary.threads, 6);
        assert!(groups[0].summary.io_rate.is_none());
        assert_eq!(groups[0].summary.status, "Verschieden");
        assert_eq!(
            groups[0]
                .indices
                .iter()
                .map(|i| (rows[*i].pid, rows[*i].started))
                .collect::<Vec<_>>(),
            vec![(2, 102), (1, 101)]
        );
    }
    #[test]
    fn expansion_categories_and_search_keep_children_and_count_original_processes() {
        let mut background = row(3);
        background.application = None;
        let rows = vec![row(1), row(2), background];
        let groups = groups(&[1, 0, 2], &rows);
        let mut opened = HashSet::new();
        assert_eq!(
            entries(&groups, &opened, true, true, None),
            vec![
                Entry::Heading(true, 2),
                Entry::Group(0),
                Entry::Heading(false, 1),
                Entry::Process(2, false)
            ]
        );
        opened.insert(groups[0].key.clone());
        assert_eq!(
            entries(&groups, &opened, true, true, Some(true)),
            vec![
                Entry::Heading(true, 2),
                Entry::Group(0),
                Entry::Process(1, true),
                Entry::Process(0, true)
            ]
        );
        assert_eq!(
            entries(&groups, &opened, false, true, None),
            vec![
                Entry::Heading(true, 2),
                Entry::Heading(false, 1),
                Entry::Process(2, false)
            ]
        );
        let filtered = super::groups(&[1], &rows);
        assert_eq!(filtered[0].summary.memory, 100);
        assert_eq!(
            entries(&filtered, &opened, true, true, Some(true)),
            vec![Entry::Heading(true, 1), Entry::Process(1, false)]
        );
    }
}
