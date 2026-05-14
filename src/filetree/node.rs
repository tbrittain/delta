use std::collections::HashSet;
use std::path::{Component, PathBuf};

use crate::diff::ChangedFile;

use super::TreeItem;

// ── Internal tree types ───────────────────────────────────────────────────────

pub(super) struct DirNode {
    pub(super) name:     String,
    pub(super) path:     PathBuf,
    pub(super) children: Vec<Node>,
}

pub(super) enum Node {
    Dir(DirNode),
    File(usize), // index into files slice
}

// ── Tree construction ─────────────────────────────────────────────────────────

pub(super) fn insert_file(
    nodes: &mut Vec<Node>,
    file_idx: usize,
    components: &[Component],
    parent: &PathBuf,
) {
    match components {
        [] => {}
        [_leaf] => nodes.push(Node::File(file_idx)),
        [dir_comp, rest @ ..] => {
            let name     = dir_comp.as_os_str().to_string_lossy().to_string();
            let dir_path = parent.join(&name);

            for node in nodes.iter_mut() {
                if let Node::Dir(d) = node {
                    if d.name == name {
                        let p = d.path.clone();
                        insert_file(&mut d.children, file_idx, rest, &p);
                        return;
                    }
                }
            }
            let mut new_dir = DirNode { name, path: dir_path.clone(), children: Vec::new() };
            insert_file(&mut new_dir.children, file_idx, rest, &dir_path);
            nodes.push(Node::Dir(new_dir));
        }
    }
}

pub(super) fn sort_nodes(nodes: &mut Vec<Node>, files: &[ChangedFile]) {
    nodes.sort_by(|a, b| match (a, b) {
        (Node::Dir(da), Node::Dir(db)) => da.name.cmp(&db.name),
        (Node::Dir(_),  Node::File(_)) => std::cmp::Ordering::Less,
        (Node::File(_), Node::Dir(_))  => std::cmp::Ordering::Greater,
        (Node::File(ia), Node::File(ib)) =>
            files[*ia].path.file_name().cmp(&files[*ib].path.file_name()),
    });
    for node in nodes.iter_mut() {
        if let Node::Dir(d) = node { sort_nodes(&mut d.children, files); }
    }
}

pub(super) fn count_files(nodes: &[Node]) -> usize {
    nodes.iter().map(|n| match n {
        Node::File(_) => 1,
        Node::Dir(d)  => count_files(&d.children),
    }).sum()
}

pub(super) fn dir_has_notes(
    nodes: &[Node],
    files: &[ChangedFile],
    noted: &HashSet<PathBuf>,
) -> bool {
    nodes.iter().any(|n| match n {
        Node::File(idx) => noted.contains(&files[*idx].path),
        Node::Dir(d)    => dir_has_notes(&d.children, files, noted),
    })
}

pub(super) fn flatten(
    nodes: &[Node],
    files: &[ChangedFile],
    noted: &HashSet<PathBuf>,
    collapsed: &HashSet<PathBuf>,
    depth: usize,
    result: &mut Vec<TreeItem>,
) {
    for node in nodes {
        match node {
            Node::Dir(d) => {
                let is_collapsed = collapsed.contains(&d.path);
                result.push(TreeItem::Dir {
                    path:         d.path.clone(),
                    display_name: format!("{}/", d.name),
                    depth,
                    file_count:   count_files(&d.children),
                    has_notes:    dir_has_notes(&d.children, files, noted),
                    collapsed:    is_collapsed,
                });
                if !is_collapsed {
                    flatten(&d.children, files, noted, collapsed, depth + 1, result);
                }
            }
            Node::File(idx) => {
                let file = &files[*idx];
                let display_name = file.path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| file.path.display().to_string());
                result.push(TreeItem::File {
                    file_idx:  *idx,
                    display_name,
                    depth,
                    has_notes: noted.contains(&file.path),
                });
            }
        }
    }
}
