//! Execution of protocol calls; wire data remains independent of the workspace.
use crate::protocol::Response;
use crate::{Answer, Call, Engine, EngineError, Reply};

impl Call {
    /// Result-byte budgets apply only to read-only commands. Mutation receipts
    /// cannot be discarded after an operation has written to the workspace.
    pub fn execute(self, engine: &Engine) -> Reply<Answer> {
        self.execute_in(engine)
    }

    /// Execute one read or explicit validation call with a single-use cooperative cancellation handle.
    pub fn execute_with_cancellation(
        self,
        engine: &Engine,
        cancellation: &crate::ReadCancellation,
    ) -> Reply<Answer> {
        let result = if !self.request.is_cancellable() {
            Err(EngineError::MutationCancellation)
        } else {
            cancellation.claim()
        };
        if let Err(error) = result {
            return Reply {
                id: self.id,
                response: Response::error(crate::Failure::from(&error)),
            };
        }
        self.execute_in(&engine.with_cancellation(cancellation.clone()))
    }

    fn execute_in(mut self, engine: &Engine) -> Reply<Answer> {
        let result = (|| {
            if let Some(max_bytes) = self.max_output_bytes {
                if !(crate::ContextBudget::MIN_BYTES..=crate::ContextBudget::MAX_BYTES)
                    .contains(&max_bytes)
                {
                    return Err(EngineError::InvalidBudget);
                }
                if !self.request.is_read_only() {
                    return Err(EngineError::MutationBudget);
                }
            }
            if let (Some(limit), crate::Request::Context(query)) =
                (self.max_output_bytes, &mut self.request)
            {
                query.budget.max_bytes = query.budget.max_bytes.min(limit);
            }
            if let Some(limit) = self.max_output_bytes {
                match &mut self.request {
                    crate::Request::PrepareRewrite(query) => {
                        crate::PlanReviewReply::budget(query.max_bytes, query.page.as_ref())?;
                        query.max_bytes = query.max_bytes.min(limit);
                    }
                    crate::Request::ReviewPlan(query) => {
                        query.page.validate()?;
                        query.page.max_bytes = query.page.max_bytes.min(limit);
                    }
                    crate::Request::PrepareRename(query) => {
                        crate::PageBudget {
                            max_bytes: query.max_bytes,
                            max_items: 1,
                        }
                        .validate()?;
                        query.max_bytes = query.max_bytes.min(limit);
                    }
                    crate::Request::InspectPlan(query) => {
                        crate::PageBudget {
                            max_bytes: query.max_bytes,
                            max_items: 1,
                        }
                        .validate()?;
                        query.max_bytes = query.max_bytes.min(limit);
                    }
                    crate::Request::Relationships(query) => {
                        query.budget.validate()?;
                        query.budget.max_bytes = query.budget.max_bytes.min(limit);
                    }
                    crate::Request::SearchPage(query) => {
                        query.page.validate()?;
                        query.page.max_bytes = query.page.max_bytes.min(limit);
                    }
                    crate::Request::ContextPage(query) => {
                        query.page.validate()?;
                        query.page.max_bytes = query.page.max_bytes.min(limit);
                    }
                    crate::Request::Continue(query) => {
                        query.page.validate()?;
                        query.page.max_bytes = query.page.max_bytes.min(limit);
                    }
                    crate::Request::Expand(query) => {
                        crate::PageBudget {
                            max_items: 1,
                            max_bytes: query.max_bytes,
                        }
                        .validate()?;
                        query.max_bytes = query.max_bytes.min(limit);
                    }
                    _ => {}
                }
            }
            let answer = engine.run(self.request)?.into_answer();
            if let Some(max_bytes) = self.max_output_bytes {
                let required_bytes = serde_json::to_vec(&answer)
                    .expect("answer serializes")
                    .len();
                if required_bytes > max_bytes {
                    return Err(EngineError::OutputLimit {
                        max_bytes,
                        required_bytes,
                    });
                }
            }
            engine.publish_read(|| Ok(answer))
        })();
        Reply {
            id: self.id,
            response: match result {
                Ok(answer) => Response::ok(answer),
                Err(error) => Response::error(crate::Failure::from(&error)),
            },
        }
    }
}
