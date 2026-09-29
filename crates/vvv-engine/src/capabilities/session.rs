//! Execution of protocol calls; wire data remains independent of the workspace.
use crate::protocol::Response;
use crate::{Answer, Call, Engine, EngineError, Reply};

impl Call {
    /// Result-byte budgets apply only to read-only commands. Mutation receipts
    /// cannot be discarded after an operation has written to the workspace.
    pub fn execute(mut self, engine: &Engine) -> Reply<Answer> {
        let result = (|| {
            if let Some(max_bytes) = self.max_output_bytes {
                if !(1024..=1_048_576).contains(&max_bytes) {
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
            Ok(answer)
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
