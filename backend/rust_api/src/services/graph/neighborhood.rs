//! 实体关系子图:从根实体 BFS N 跳,返回节点与有向边。
//! 读取时现算,不写库、不调 LLM;供概念面板的图谱视图使用。
//! ponytail: 每节点只展开 PER_NODE_CAP 条最强边,够画局部图;要看全图再上分页/游标。

use std::collections::HashSet;

use crate::db::Db;
use crate::error::AppError;
use crate::models::api::{EntityNeighborhoodView, EntitySummary, NeighborhoodEdge};

/// 每个节点最多展开的邻居数(related_entities 已按 confidence 降序,
/// 取到的都是最强的边),防止 hub 节点把子图撑爆。
const PER_NODE_CAP: u32 = 12;

/// 从 root 起 BFS 最多 `depth` 跳(内部 clamp 1..=2),节点总数不超过 `limit`。
pub fn entity_neighborhood(
    db: &Db,
    root_id: &str,
    depth: u32,
    limit: u32,
) -> Result<EntityNeighborhoodView, AppError> {
    db.get_entity(root_id)
        .map_err(|_| AppError::not_found(format!("entity not found: {root_id}")))?;
    let root = db
        .entity_summary(root_id)?
        .ok_or_else(|| AppError::not_found(format!("entity not found: {root_id}")))?;

    let depth = depth.clamp(1, 2);
    let limit = limit.max(1) as usize;
    let mut nodes: Vec<EntitySummary> = vec![root];
    let mut visited: HashSet<String> = HashSet::from([root_id.to_string()]);
    let mut edges: Vec<NeighborhoodEdge> = Vec::new();
    let mut seen_edges: HashSet<(String, String, String)> = HashSet::new();
    let mut frontier: Vec<String> = vec![root_id.to_string()];

    'outer: for _ in 0..depth {
        if frontier.is_empty() {
            break;
        }
        let mut next: Vec<String> = Vec::new();
        for node_id in frontier.drain(..) {
            for neighbor in db.related_entities(&node_id, None, PER_NODE_CAP)? {
                if neighbor.entity_id == node_id {
                    continue;
                }
                let (from, to) = if neighbor.direction == "out" {
                    (node_id.clone(), neighbor.entity_id.clone())
                } else {
                    (neighbor.entity_id.clone(), node_id.clone())
                };
                if seen_edges.insert((from.clone(), to.clone(), neighbor.relation_type.clone())) {
                    edges.push(NeighborhoodEdge {
                        from_entity_id: from,
                        to_entity_id: to,
                        relation_type: neighbor.relation_type,
                        confidence: neighbor.confidence,
                        explanation: neighbor.explanation,
                        source_document_id: neighbor.source_document_id,
                    });
                }
                if !visited.insert(neighbor.entity_id.clone()) {
                    continue;
                }
                if nodes.len() >= limit {
                    break 'outer;
                }
                nodes.push(EntitySummary {
                    entity_id: neighbor.entity_id.clone(),
                    name: neighbor.name,
                    entity_type: neighbor.entity_type,
                    aliases: neighbor.aliases,
                    mention_count: neighbor.mention_count,
                    document_count: neighbor.document_count,
                });
                next.push(neighbor.entity_id);
            }
        }
        frontier = next;
    }

    // 截断后可能留下端点不在节点集里的边,过滤掉。
    let node_ids: HashSet<&str> = nodes.iter().map(|node| node.entity_id.as_str()).collect();
    edges.retain(|edge| {
        node_ids.contains(edge.from_entity_id.as_str())
            && node_ids.contains(edge.to_entity_id.as_str())
    });

    Ok(EntityNeighborhoodView {
        root: root_id.to_string(),
        nodes,
        edges,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;

    use crate::db::Db;
    use crate::models::api::{NewEntity, NewEntityRelation};

    use super::*;

    struct Fixture {
        root: PathBuf,
        db: Db,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn fixture(name: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "graph-neighborhood-{name}-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(root.join("data")).expect("data root");
        fs::create_dir_all(root.join("db")).expect("db dir");
        let db = Db::new(root.join("db").join("jobs.db"), root.join("data"));
        db.init().expect("init");
        Fixture { root, db }
    }

    fn entity(db: &Db, name: &str) -> String {
        db.upsert_entity(&NewEntity {
            name: name.to_string(),
            entity_type: "concept".to_string(),
            aliases: Vec::new(),
            description: String::new(),
        })
        .expect("entity")
        .entity_id
    }

    fn rel(from: &str, to: &str, kind: &str) -> NewEntityRelation {
        NewEntityRelation {
            from_entity_id: from.to_string(),
            to_entity_id: to.to_string(),
            relation_type: kind.to_string(),
            confidence: 0.9,
            explanation: "依据".to_string(),
            source_document_id: String::new(),
            source_block_id: String::new(),
        }
    }

    fn node_ids(view: &EntityNeighborhoodView) -> Vec<String> {
        view.nodes.iter().map(|node| node.entity_id.clone()).collect()
    }

    #[test]
    fn depth_bounds_the_frontier() {
        let fx = fixture("depth");
        let (a, b, c, d) = (
            entity(&fx.db, "A"),
            entity(&fx.db, "B"),
            entity(&fx.db, "C"),
            entity(&fx.db, "D"),
        );
        for (from, to) in [(&a, &b), (&b, &c), (&c, &d)] {
            fx.db.add_entity_relation(&rel(from, to, "uses")).expect("rel");
        }

        let one = entity_neighborhood(&fx.db, &a, 1, 50).expect("depth 1");
        assert_eq!(node_ids(&one), vec![a.clone(), b.clone()]);
        assert_eq!(one.root, a);

        let two = entity_neighborhood(&fx.db, &a, 2, 50).expect("depth 2");
        assert_eq!(two.nodes.len(), 3);
        assert!(node_ids(&two).contains(&c));
        assert!(!node_ids(&two).contains(&d), "D 是三跳外");

        // depth 超过上限被 clamp 到 2
        let deep = entity_neighborhood(&fx.db, &a, 9, 50).expect("clamped");
        assert_eq!(deep.nodes.len(), 3);
    }

    #[test]
    fn edges_keep_direction_from_to() {
        let fx = fixture("direction");
        let (a, b, c) = (
            entity(&fx.db, "A"),
            entity(&fx.db, "B"),
            entity(&fx.db, "C"),
        );
        fx.db.add_entity_relation(&rel(&a, &b, "uses")).expect("a->b");
        fx.db.add_entity_relation(&rel(&c, &a, "evaluates")).expect("c->a");

        let view = entity_neighborhood(&fx.db, &a, 1, 50).expect("view");
        let out = view
            .edges
            .iter()
            .find(|edge| edge.from_entity_id == a && edge.to_entity_id == b)
            .expect("A->B 出边");
        assert_eq!(out.relation_type, "uses");
        let incoming = view
            .edges
            .iter()
            .find(|edge| edge.from_entity_id == c && edge.to_entity_id == a)
            .expect("C->A 入边");
        assert_eq!(incoming.relation_type, "evaluates");
        // 不返回相对方向,只保留 from/to 表达
        assert!(view
            .edges
            .iter()
            .all(|edge| !(edge.from_entity_id == b && edge.to_entity_id == a)));
    }

    #[test]
    fn edge_dedupes_across_layers() {
        let fx = fixture("dedupe");
        let (a, b) = (entity(&fx.db, "A"), entity(&fx.db, "B"));
        fx.db.add_entity_relation(&rel(&a, &b, "uses")).expect("a->b");

        // 第二跳从 B 反查会再遇到同一条 A->B 边,必须去重。
        let view = entity_neighborhood(&fx.db, &a, 2, 50).expect("view");
        let count = view
            .edges
            .iter()
            .filter(|edge| {
                edge.from_entity_id == a
                    && edge.to_entity_id == b
                    && edge.relation_type == "uses"
            })
            .count();
        assert_eq!(count, 1, "edges: {:?}", view.edges.len());
    }

    #[test]
    fn self_loop_is_skipped() {
        let fx = fixture("self-loop");
        let a = entity(&fx.db, "A");
        fx.db.add_entity_relation(&rel(&a, &a, "related_to")).expect("self");

        let view = entity_neighborhood(&fx.db, &a, 2, 50).expect("view");
        assert_eq!(view.nodes.len(), 1);
        assert!(view.edges.is_empty());
    }

    #[test]
    fn limit_truncates_and_drops_dangling_edges() {
        let fx = fixture("limit");
        let root = entity(&fx.db, "root");
        for name in ["b1", "b2", "b3", "b4", "b5"] {
            let leaf = entity(&fx.db, name);
            fx.db.add_entity_relation(&rel(&root, &leaf, "uses")).expect("rel");
        }

        // root + 2 邻居;第三条边在截断前已入队,收尾必须过滤掉。
        let view = entity_neighborhood(&fx.db, &root, 1, 3).expect("view");
        assert_eq!(view.nodes.len(), 3);
        assert_eq!(view.edges.len(), 2, "edges: {:?}", view.edges);
        let ids: HashSet<&str> = view.nodes.iter().map(|node| node.entity_id.as_str()).collect();
        for edge in &view.edges {
            assert!(ids.contains(edge.from_entity_id.as_str()));
            assert!(ids.contains(edge.to_entity_id.as_str()));
        }
    }

    #[test]
    fn unknown_root_is_not_found() {
        let fx = fixture("missing");
        assert!(entity_neighborhood(&fx.db, "ent-nope", 2, 50).is_err());
    }
}
