//! Record workflows shared by tenant transport and in-memory regression tests.
//! Guards, rollback eligibility and unlink loops live here; conversion_cli
//! supplies reads, sends and durable persistence below those decisions.

use serde_json::Value;

use super::conversion::{self, Action, Capture, Census, Link, Observation, Pair};
use super::{api, relink, repair, storage};
use crate::{Error, Result};

pub(super) trait Read {
    async fn config(&self) -> Result<(Value, Value)>;
    async fn scan(&self, object: &str, key: &str) -> Result<Vec<Value>>;
    async fn activate(&self, pair: &Pair) -> Result<()>;
}

pub(super) async fn census(io: &impl Read, pair: &Pair) -> Result<Census> {
    let forward = io.scan(&pair.forward.object, &pair.forward.key).await?;
    let target = pair
        .forward
        .target
        .strip_prefix("managed/")
        .ok_or_else(|| Error::Config("invalid capture target".into()))?;
    let key = pair.reverse.as_ref().map_or("_id", |end| end.key.as_str());
    conversion::census(pair, &forward, &io.scan(target, key).await?)
}

pub(super) async fn ready(io: &impl Read, pair: &Pair) -> Result<()> {
    let (managed, repo) = io.config().await?;
    conversion::guard(pair, &managed, &repo, storage::Mode::Mapped)?;
    conversion::subset(pair, &census(io, pair).await?)?;
    if !pair.links.is_empty() {
        io.activate(pair).await?;
    }
    Ok(())
}

pub(super) trait Removal: Read {
    async fn observe(&self, pair: &Pair, link: &Link) -> Result<Observation>;
    async fn check(&self, pair: &Pair, link: &Link, edge: &relink::Edge) -> Result<()>;
    async fn present(&self, pair: &Pair, link: &Link) -> Result<bool>;
    async fn save(&self, capture: &Capture) -> Result<()>;
    async fn send(&self, pair: &Pair, write: &relink::Write) -> Result<()>;
    async fn write_config(&self, pair: &Pair, plan: &repair::Plan) -> Result<()>;
}

fn removal(
    capture: &Capture,
    pair_index: usize,
    link_index: usize,
    observation: &Observation,
    permit: &relink::RelinkPermit,
) -> Result<Option<relink::Write>> {
    let pair = &capture.pairs[pair_index];
    let link = &pair.links[link_index];
    let Some(edge) = observation.edges.iter().find(|edge| link.matches(edge)) else {
        return Ok(None);
    };
    if edge.link.edge_id.is_none() {
        return Err(Error::Config(
            "mapped link has no edge ID; cannot remove it safely".into(),
        ));
    }
    let owned = capture
        .restored
        .iter()
        .any(|receipt| receipt.pair == pair_index && receipt.link == link_index)
        || capture
            .pending
            .as_ref()
            .is_some_and(|intent| intent.pair == pair_index && intent.link == link_index);
    if !owned {
        return Err(Error::Config(format!(
            "{} was not recorded as restored by this conversion; inspect it before rollback",
            link.label()
        )));
    }
    let [remove, _] = relink::writes(
        permit,
        &pair.forward.object,
        &pair.forward.key,
        pair.forward.array,
        edge,
        observation.rev.as_deref(),
    )
    .map_err(Error::Config)?;
    if !pair.forward.array && remove.if_match.is_none() {
        return Err(Error::Config(
            "rollback requires a record _rev for has-one removal".into(),
        ));
    }
    Ok(Some(remove))
}

fn removable(managed: &Value, pair: &Pair) -> Result<()> {
    if api::object_named(&managed, &pair.forward.object)?
        .pointer("/schema/required")
        .and_then(Value::as_array)
        .is_some_and(|required| {
            required
                .iter()
                .any(|key| key.as_str() == Some(&pair.forward.key))
        })
    {
        return Err(Error::Config(format!(
            "{} is required; explicitly clear required before rollback, then reinstate it afterwards",
            pair.forward.label()
        )));
    }
    Ok(())
}

/// Complete preflight happens before config consent proof, state change or
/// unlink: external dependents, required fields, ownership, revisions, far
/// resolution and runtime are all checked for the entire batch.
async fn preflight(
    io: &impl Removal,
    capture: &Capture,
    permit: &relink::RelinkPermit,
) -> Result<()> {
    let (managed, repo) = io.config().await?;
    for (index, pair) in capture.pairs.iter().enumerate() {
        repair::plan_rollback(&managed, &repo, pair).map_err(Error::Config)?;
        removable(&managed, pair)?;
        ready(io, pair).await?;
        for (link_index, link) in pair.links.iter().enumerate() {
            let observation = io.observe(pair, link).await?;
            if removal(capture, index, link_index, &observation, permit)?.is_some() {
                let edge = observation
                    .edges
                    .iter()
                    .find(|edge| link.matches(edge))
                    .ok_or_else(|| Error::Config("rollback observation changed".into()))?;
                io.check(pair, link, edge).await?;
            }
        }
    }
    Ok(())
}

async fn remove_links(
    io: &impl Removal,
    capture: &mut Capture,
    permit: &relink::RelinkPermit,
) -> Result<()> {
    if let Some(intent) = capture
        .pending
        .clone()
        .filter(|intent| intent.action == Action::Create)
    {
        let pair = &capture.pairs[intent.pair];
        let link = &pair.links[intent.link];
        let current = io.observe(pair, link).await?;
        if let Some(edge) = current.edges.iter().find(|edge| link.matches(edge)) {
            if !io.present(pair, link).await? {
                return Err(Error::Config(
                    "pending link is not proven in mapped storage; inspect before rollback".into(),
                ));
            }
            conversion::receipt(capture, intent.pair, intent.link, edge)?;
        }
        capture.pending = None;
        io.save(capture).await?;
    }
    for index in 0..capture.pairs.len() {
        let pair = capture.pairs[index].clone();
        ready(io, &pair).await?;
        for (link_index, link) in pair.links.iter().enumerate() {
            // Retain the fresh config/removal guards at every request too.
            let (managed, repo) = io.config().await?;
            repair::plan_rollback(&managed, &repo, &pair).map_err(Error::Config)?;
            removable(&managed, &pair)?;
            let current = io.observe(&pair, link).await?;
            let Some(write) = removal(capture, index, link_index, &current, permit)? else {
                continue;
            };
            let edge = current
                .edges
                .iter()
                .find(|edge| link.matches(edge))
                .ok_or_else(|| Error::Config("rollback observation changed".into()))?;
            io.check(&pair, link, edge).await?;
            capture.pending = Some(conversion::Intent {
                pair: index,
                link: link_index,
                action: Action::Remove,
                edge_id: edge.link.edge_id.clone(),
            });
            io.save(capture).await?;
            io.send(&pair, &write).await?;
            if io
                .observe(&pair, link)
                .await?
                .edges
                .iter()
                .any(|edge| link.matches(edge))
            {
                return Err(Error::Config(format!(
                    "{} removal not confirmed; re-run rollback-map",
                    link.label()
                )));
            }
            capture.pending = None;
            io.save(capture).await?;
        }
        if !census(io, &pair).await?.links.is_empty() {
            return Err(Error::Config(
                "mapped links remain; mappings were not removed".into(),
            ));
        }
    }
    Ok(())
}

pub(super) async fn rollback(
    io: &impl Removal,
    capture: &mut Capture,
    permit: &conversion::Permit,
    record_permit: &relink::RelinkPermit,
) -> Result<()> {
    if capture.origin != conversion::Origin::InPlace || capture.pairs.len() != 1 {
        return Err(Error::Config(
            "rollback-map requires one in-place captured pair".into(),
        ));
    }
    let pair = capture.pairs[0].clone();
    preflight(io, capture, record_permit).await?;
    conversion::rollback_with(
        capture,
        permit,
        async || {
            let (managed, repo) = io.config().await?;
            let plan = repair::plan_confirm(&managed, &repo, &pair).map_err(Error::Config)?;
            io.write_config(&pair, &plan).await
        },
        async |capture| io.save(capture).await,
        async |capture| remove_links(io, capture, record_permit).await,
        async || {
            if !census(io, &pair).await?.links.is_empty() {
                return Err(Error::Config(
                    "mapped store is not empty; refusing to remove mappings".into(),
                ));
            }
            let (managed, repo) = io.config().await?;
            let plan = repair::plan_rollback(&managed, &repo, &pair).map_err(Error::Config)?;
            io.write_config(&pair, &plan).await
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use serde_json::json;

    use super::*;
    use crate::managed::conversion::State;
    use crate::managed::conversion::tests::{captured, fixture, records};
    use crate::managed::verify;

    struct Memory {
        managed: Value,
        repo: RefCell<Value>,
        forward: RefCell<Vec<Value>>,
        reverse: RefCell<Vec<Value>>,
        journal: RefCell<Vec<Capture>>,
        events: RefCell<Vec<String>>,
    }

    impl Memory {
        fn new(array: bool) -> (Self, Capture) {
            let (managed, _, repo, _) = fixture(array);
            let (forward, reverse) = records(array);
            let (mut capture, current) = captured(array);
            capture.state = State::Complete;
            conversion::receipt(&mut capture, 0, 0, &current.edges[0]).unwrap();
            (
                Self {
                    managed,
                    repo: RefCell::new(repo),
                    forward: RefCell::new(forward),
                    reverse: RefCell::new(reverse),
                    journal: RefCell::new(Vec::new()),
                    events: RefCell::new(Vec::new()),
                },
                capture,
            )
        }
    }

    impl Read for Memory {
        async fn config(&self) -> Result<(Value, Value)> {
            Ok((self.managed.clone(), self.repo.borrow().clone()))
        }
        async fn scan(&self, object: &str, _: &str) -> Result<Vec<Value>> {
            Ok(if object == "a" {
                self.forward.borrow().clone()
            } else {
                self.reverse.borrow().clone()
            })
        }
        async fn activate(&self, _: &Pair) -> Result<()> {
            Ok(())
        }
    }
    impl Removal for Memory {
        async fn observe(&self, pair: &Pair, link: &Link) -> Result<Observation> {
            let records = self.forward.borrow();
            let record = records
                .iter()
                .find(|record| record["_id"] == link.from_id)
                .unwrap();
            Ok(Observation {
                edges: relink::edges_of(record, &pair.forward.key).map_err(Error::Config)?,
                rev: Some("r1".into()),
            })
        }
        async fn check(&self, _: &Pair, _: &Link, edge: &relink::Edge) -> Result<()> {
            let far = verify::links_of(&self.reverse.borrow()[0], "back").map_err(Error::Config)?;
            assert_eq!(
                verify::judge(&edge.link, "managed/a", Some(&far)),
                verify::LinkOutcome::Resolved
            );
            Ok(())
        }
        async fn present(&self, _: &Pair, _: &Link) -> Result<bool> {
            Ok(true)
        }
        async fn save(&self, capture: &Capture) -> Result<()> {
            self.journal.borrow_mut().push(capture.clone());
            Ok(())
        }
        async fn send(&self, pair: &Pair, write: &relink::Write) -> Result<()> {
            let journal = self.journal.borrow();
            let pending = journal.last().unwrap().pending.as_ref().unwrap();
            assert_eq!(pending.action, Action::Remove);
            assert_eq!(journal.last().unwrap().state, State::RollingBack);
            if pair.forward.array {
                assert_eq!(write.method, "DELETE");
                assert!(write.path.ends_with("/link/e1"), "{}", write.path);
                self.forward.borrow_mut()[0]["link"] = json!([]);
            } else {
                assert_eq!(write.method, "PATCH");
                assert_eq!(write.body.as_ref().unwrap()[0]["operation"], "remove");
                assert!(write.if_match.is_some());
                self.forward.borrow_mut()[0]
                    .as_object_mut()
                    .unwrap()
                    .remove("link");
            }
            self.reverse.borrow_mut()[0]["back"] = json!([]);
            self.events.borrow_mut().push("unlink".into());
            Ok(())
        }
        async fn write_config(&self, _: &Pair, plan: &repair::Plan) -> Result<()> {
            if plan.after() == &*self.repo.borrow() {
                self.events.borrow_mut().push("confirm config write".into());
            } else {
                assert!(
                    relink::edges_of(&self.forward.borrow()[0], "link")
                        .unwrap()
                        .is_empty()
                );
                assert!(self.journal.borrow().last().unwrap().pending.is_none());
                self.events.borrow_mut().push("remove mapping".into());
            }
            *self.repo.borrow_mut() = plan.after().clone();
            Ok(())
        }
    }

    // Regression: the old test supplied the guard itself and could not catch
    // removal of the production ready guard. Only config/record reads vary here.
    #[tokio::test]
    async fn restore_refuses_unmapped_upper_before_any_record_write() {
        let (io, mut capture) = Memory::new(true);
        *io.repo.borrow_mut() = fixture(true).1;
        let writes = RefCell::new(0);
        let error = conversion::restore_with(
            &mut capture,
            &conversion::authorize(false, true).unwrap(),
            async |pair| ready(&io, pair).await,
            async |pair, link| io.observe(pair, link).await,
            async |capture| io.save(capture).await,
            async |_, _, _| {
                *writes.borrow_mut() += 1;
                Ok(None)
            },
            async |pair, link, edge| io.check(pair, link, edge).await,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("Mapped"), "{error}");
        assert_eq!(*writes.borrow(), 0);
        assert!(io.journal.borrow().is_empty());
    }

    // Regression: dependencies used to be discovered after unlinking. Exercise
    // full production preflight and removal with raw config/record transport.
    #[tokio::test]
    async fn rollback_external_dependent_causes_zero_record_or_config_writes() {
        let (io, mut capture) = Memory::new(true);
        io.repo.borrow_mut()["resourceMapping"]["explicitMapping"]["internal/role"]["properties"]
            ["dependent"] = json!({"type":"reverseReference","resourcePath":"managed/a","propertyName":"link","isMultiValued":true});
        let error = rollback(
            &io,
            &mut capture,
            &conversion::authorize(false, true).unwrap(),
            &relink::authorize(false, true).unwrap(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("dependent"), "{error}");
        assert!(io.events.borrow().is_empty());
        assert!(io.journal.borrow().is_empty());
        assert_eq!(census(&io, &capture.pairs[0]).await.unwrap().links.len(), 1);
        assert_eq!(capture.state, State::Complete);
    }

    // The transport applies real DELETE/PATCH removals; the final production
    // plan cannot pass until the mapped store is empty. Covers both shapes.
    #[tokio::test]
    async fn rollback_removes_restored_links_before_mapping_and_rejects_promotion() {
        for array in [false, true] {
            let (io, mut capture) = Memory::new(array);
            rollback(
                &io,
                &mut capture,
                &conversion::authorize(false, true).unwrap(),
                &relink::authorize(false, true).unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(
                *io.events.borrow(),
                ["confirm config write", "unlink", "remove mapping"]
            );
            assert!(
                relink::edges_of(&io.forward.borrow()[0], "link")
                    .unwrap()
                    .is_empty()
            );
            assert!(
                relink::edges_of(&io.reverse.borrow()[0], "back")
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                storage::mode(&storage::pair(&io.managed, &io.repo.borrow(), "a", "link").unwrap()),
                storage::Mode::Generic
            );
            assert!(capture.pending.is_none());
        }
        let (io, mut capture) = Memory::new(true);
        capture.origin = conversion::Origin::Promotion;
        assert!(
            rollback(
                &io,
                &mut capture,
                &conversion::authorize(false, true).unwrap(),
                &relink::authorize(false, true).unwrap()
            )
            .await
            .is_err()
        );
        assert!(io.events.borrow().is_empty());
    }

    #[tokio::test]
    async fn rollback_required_property_refuses_before_any_unlink() {
        let (mut io, mut capture) = Memory::new(false);
        io.managed["objects"][0]["schema"]["required"] = json!(["link"]);
        let error = rollback(
            &io,
            &mut capture,
            &conversion::authorize(false, true).unwrap(),
            &relink::authorize(false, true).unwrap(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("required"));
        assert!(io.events.borrow().is_empty());
        assert!(io.journal.borrow().is_empty());
    }

    #[tokio::test]
    async fn rolling_back_capture_can_resume_record_only_restore() {
        let (io, mut capture) = Memory::new(true);
        capture.state = State::RollingBack;
        capture.pending = Some(conversion::Intent {
            pair: 0,
            link: 0,
            action: Action::Remove,
            edge_id: Some("e1".into()),
        });
        io.forward.borrow_mut()[0]["link"] = json!([]);
        io.reverse.borrow_mut()[0]["back"] = json!([]);
        let writes = RefCell::new(0);
        conversion::restore_with(
            &mut capture,
            &conversion::authorize(false, true).unwrap(),
            async |pair| ready(&io, pair).await,
            async |pair, link| io.observe(pair, link).await,
            async |capture| io.save(capture).await,
            async |_, _, _| {
                let (forward, reverse) = records(true);
                *io.forward.borrow_mut() = forward;
                *io.reverse.borrow_mut() = reverse;
                *writes.borrow_mut() += 1;
                Ok(Some("e1".into()))
            },
            async |pair, link, edge| io.check(pair, link, edge).await,
        )
        .await
        .unwrap();
        conversion::exact(
            &capture.pairs[0],
            &census(&io, &capture.pairs[0]).await.unwrap(),
        )
        .unwrap();
        assert_eq!(*writes.borrow(), 1);
        assert!(capture.pending.is_none());
        assert_eq!(capture.state, State::Restoring);
        assert!(io.events.borrow().is_empty()); // records only; no config write
    }
}
