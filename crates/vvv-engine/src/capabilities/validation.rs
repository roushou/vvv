//! Explicit check commands and evidence tied to an applied plan's source versions.
pub(crate) mod baseline;
mod process;
use crate::graph::query_snapshot::QuerySnapshot;
use crate::{
    ContentId, Engine, EngineError, PlanId, PlanReceipt, RelPath, SnapshotId, SourceVersion,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CheckCommand {
    /// A caller-defined label, such as formatting, compilation, or unit tests.
    pub name: String,
    /// Executed directly, without implicit shell parsing.
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct ValidationBudget {
    /// Deadline for the command batch, including between-command captures.
    /// Initial and final input capture and process cleanup are outside this limit.
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = 300000)))]
    pub timeout_ms: u64,
    #[cfg_attr(feature = "schema", schemars(range(min = 4096, max = 1048576)))]
    pub max_bytes: usize,
}
impl Default for ValidationBudget {
    fn default() -> Self {
        Self {
            timeout_ms: 60_000,
            max_bytes: 16_384,
        }
    }
}
impl ValidationBudget {
    fn validate(&self) -> Result<(), EngineError> {
        if !(1..=300_000).contains(&self.timeout_ms)
            || !(4096..=1_048_576).contains(&self.max_bytes)
        {
            return Err(EngineError::InvalidBudget);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ValidatePlanQuery {
    pub plan_id: PlanId,
    #[cfg_attr(feature = "schema", schemars(length(min = 1, max = 4)))]
    pub checks: Vec<CheckCommand>,
    /// Explicit hidden/ignored inputs, in addition to workspace-visible files.
    #[serde(default)]
    #[cfg_attr(feature = "schema", schemars(length(max = 32)))]
    pub extra_inputs: Vec<RelPath>,
    #[serde(default)]
    pub budget: ValidationBudget,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ValidationReport {
    pub plan_id: PlanId,
    pub history_id: u64,
    /// Monotonically increasing within this retained plan.
    pub run: u64,
    pub sources: Vec<SourceVersion>,
    pub before: SnapshotId,
    pub after: Option<SnapshotId>,
    pub input_files: usize,
    pub extra_inputs: Vec<RelPath>,
    pub source_state: ValidationSourceState,
    /// True only when every command succeeded and observed inputs stayed unchanged.
    pub passed: bool,
    pub checks: Vec<CheckResult>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ValidationSourceState {
    Unchanged,
    Changed,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    NotRun,
    Passed,
    Failed,
    TimedOut,
    Cancelled,
    Error,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CheckResult {
    pub command: CheckCommand,
    pub outcome: CheckOutcome,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout: CheckOutput,
    pub stderr: CheckOutput,
    pub failure: Option<CheckFailure>,
}
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CheckOutput {
    pub text: String,
    pub bytes_seen: u64,
    pub truncated: bool,
    /// False when capture stopped before EOF or a read failed.
    pub complete: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CheckFailure {
    pub operation: CheckOperation,
    pub os_code: Option<i32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CheckOperation {
    Spawn,
    Capture,
    Wait,
    Terminate,
}
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd"
))]
impl CheckFailure {
    fn io(operation: CheckOperation, error: std::io::Error) -> Self {
        Self {
            operation,
            os_code: error.raw_os_error(),
        }
    }
}
impl CheckResult {
    fn pending(command: CheckCommand) -> Self {
        Self {
            command,
            outcome: CheckOutcome::NotRun,
            exit_code: None,
            duration_ms: 0,
            stdout: CheckOutput::default(),
            stderr: CheckOutput::default(),
            failure: None,
        }
    }
}
impl ValidatePlanQuery {
    pub(crate) fn available(engine: &Engine) -> bool {
        process::CheckProcess::supported() && engine.workspace().execution_root().is_some()
    }
    pub fn execute(self, engine: &Engine) -> Result<ValidationReport, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &Engine) -> Result<ValidationReport, EngineError> {
        self.validate()?;
        let root = engine
            .workspace()
            .execution_root()
            .ok_or(EngineError::ValidationUnavailable)?;
        if !process::CheckProcess::supported() {
            return Err(EngineError::ValidationUnavailable);
        }
        let (receipt, baseline, run) = engine.plans().validation(&self.plan_id, Instant::now())?;
        baseline.validate(engine).map_err(|error| match error {
            EngineError::StaleQuery | EngineError::StaleSource { .. } => EngineError::StalePlan,
            other => other,
        })?;
        let before = ValidationInputs::capture(engine, &self.extra_inputs, &baseline)?;
        // Capture again before launching to reject inconsistent reads during capture.
        if !before.matches_baseline(&baseline)
            || before != ValidationInputs::capture(engine, &self.extra_inputs, &baseline)?
        {
            return Err(EngineError::StalePlan);
        }
        let mut report = ValidationReport::new(
            receipt,
            run,
            &before,
            self.extra_inputs.clone(),
            self.checks,
        );
        report.preflight(self.budget.max_bytes)?;
        engine
            .plans()
            .reserve_validation(&self.plan_id, self.budget.max_bytes)?;
        let started = Instant::now();
        let deadline = started + Duration::from_millis(self.budget.timeout_ms);
        let limit = self.budget.max_bytes / (report.checks.len() * 16);
        for check in &mut report.checks {
            if engine.cancellation().is_some_and(|c| c.is_cancelled()) {
                check.outcome = CheckOutcome::Cancelled;
                break;
            }
            if Instant::now() >= deadline {
                check.outcome = CheckOutcome::TimedOut;
                break;
            }
            // A prior check must not change the inputs seen by the next one.
            if ValidationInputs::capture(engine, &self.extra_inputs, &baseline)
                .is_ok_and(|current| current == before)
            {
                *check = process::CheckProcess::run(
                    &check.command,
                    root,
                    deadline,
                    limit,
                    engine.cancellation(),
                );
            } else {
                break;
            }
        }
        // Cancellation must not discard the evidence of commands that already ran.
        let observed = engine.without_cancellation();
        match ValidationInputs::capture(&observed, &self.extra_inputs, &baseline) {
            Ok(after) => {
                report.source_state = if after == before {
                    ValidationSourceState::Unchanged
                } else {
                    ValidationSourceState::Changed
                };
                report.after = Some(after.identity());
            }
            Err(_) => report.source_state = ValidationSourceState::Unavailable,
        }
        report.passed = report.source_state == ValidationSourceState::Unchanged
            && report
                .checks
                .iter()
                .all(|check| check.outcome == CheckOutcome::Passed);
        report.fit(self.budget.max_bytes);
        // External checks can create files; never leave a trusted graph after them.
        engine.touched();
        engine
            .plans()
            .record_validation(&self.plan_id, report.clone());
        Ok(report)
    }
    fn validate(&self) -> Result<(), EngineError> {
        self.budget.validate()?;
        if self.checks.is_empty() || self.checks.len() > 4 || self.extra_inputs.len() > 32 {
            return Err(EngineError::InvalidValidation);
        }
        let mut bytes = 0usize;
        for check in &self.checks {
            if check.name.is_empty()
                || check.name.len() > 128
                || check.program.is_empty()
                || check.program.contains('\0')
                || check.args.len() > 64
                || check.args.iter().any(|arg| arg.contains('\0'))
            {
                return Err(EngineError::InvalidValidation);
            }
            bytes = bytes
                .saturating_add(check.name.len())
                .saturating_add(check.program.len())
                .saturating_add(check.args.iter().map(String::len).sum::<usize>());
        }
        if bytes > 8192 {
            return Err(EngineError::InvalidValidation);
        }
        for path in &self.extra_inputs {
            if path.as_os_str().is_empty()
                || path.is_absolute()
                || path
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Err(EngineError::InvalidValidation);
            }
        }
        Ok(())
    }
}
#[derive(Debug, PartialEq, Eq, Serialize)]
struct ValidationInputs {
    files: BTreeMap<RelPath, ContentId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    moves: Option<baseline::MoveObservation>,
}
impl ValidationInputs {
    fn capture(
        engine: &Engine,
        extra: &[RelPath],
        baseline: &baseline::ValidationBaseline,
    ) -> Result<Self, EngineError> {
        let (_, current) = QuerySnapshot::capture(engine)?;
        let mut files = BTreeMap::new();
        for path in engine.workspace().files()? {
            engine.check_read()?;
            let relative: RelPath = engine.workspace().relative(&path).into();
            if relative
                .components()
                .next()
                .is_some_and(|c| c.as_os_str() == ".vvv")
            {
                continue;
            }
            let bytes = engine.workspace().vfs().read_bytes(&path)?;
            files.insert(relative, ContentId::of_bytes(&bytes));
        }
        for path in extra
            .iter()
            .chain(baseline.inputs().keys())
            .chain(current.inputs().keys())
        {
            engine.check_read()?;
            let bytes = engine
                .workspace()
                .vfs()
                .read_bytes(&engine.workspace().absolute(path))?;
            let content = ContentId::of_bytes(&bytes);
            if files.get(path).is_some_and(|previous| previous != &content) {
                return Err(EngineError::StalePlan);
            }
            files.insert(path.clone(), content);
        }
        Ok(Self {
            files,
            moves: baseline.observe(engine)?,
        })
    }
    fn matches_baseline(&self, baseline: &baseline::ValidationBaseline) -> bool {
        baseline
            .inputs()
            .iter()
            .all(|(path, content)| self.files.get(path) == Some(content))
            && baseline.matches(self.moves.as_ref())
    }
    fn identity(&self) -> SnapshotId {
        ContentId::of(&serde_json::to_string(self).expect("inputs serialize")).into()
    }
}
impl ValidationReport {
    fn new(
        receipt: PlanReceipt,
        run: u64,
        inputs: &ValidationInputs,
        extra_inputs: Vec<RelPath>,
        checks: Vec<CheckCommand>,
    ) -> Self {
        Self {
            plan_id: receipt.plan_id,
            history_id: receipt.history_id,
            run,
            sources: receipt.files,
            before: inputs.identity(),
            after: None,
            input_files: inputs.files.len(),
            extra_inputs,
            source_state: ValidationSourceState::Unavailable,
            passed: false,
            checks: checks.into_iter().map(CheckResult::pending).collect(),
        }
    }
    fn preflight(&self, max_bytes: usize) -> Result<(), EngineError> {
        // Reserve fixed metadata growth (exit codes, timings, failure data and after id).
        let required_bytes = serde_json::to_vec(self)
            .expect("report serializes")
            .len()
            .saturating_add(1024);
        if required_bytes > max_bytes {
            return Err(EngineError::OutputLimit {
                max_bytes,
                required_bytes,
            });
        }
        Ok(())
    }
    fn fit(&mut self, max_bytes: usize) {
        while serde_json::to_vec(self).expect("report serializes").len() > max_bytes {
            let output = self
                .checks
                .iter_mut()
                .flat_map(|check| [&mut check.stdout, &mut check.stderr])
                .max_by_key(|output| output.text.len())
                .expect("nonempty checks");
            assert!(!output.text.is_empty(), "preflight reserved metadata");
            let mut end = output.text.len() / 2;
            while !output.text.is_char_boundary(end) {
                end -= 1;
            }
            output.text.truncate(end);
            output.truncated = true;
        }
    }
}
impl crate::report::Document {
    pub(crate) fn validation(report: &ValidationReport) -> Self {
        use crate::protocol::display::{Line, Role};
        let mut doc = Self::new();
        doc.body([Line::single(
            Role::Strong,
            if report.passed {
                "Validation passed"
            } else {
                "Validation did not pass"
            },
        )]);
        for check in &report.checks {
            let status = match check.outcome {
                CheckOutcome::NotRun => "not run",
                CheckOutcome::Passed => "passed",
                CheckOutcome::Failed => "failed",
                CheckOutcome::TimedOut => "timed out",
                CheckOutcome::Cancelled => "cancelled",
                CheckOutcome::Error => "could not complete",
            };
            doc.body([Line::single(Role::Plain, &check.command.name)
                .and(Role::Plain, format!(": {status}"))]);
            doc.body(
                check
                    .stdout
                    .text
                    .lines()
                    .chain(check.stderr.text.lines())
                    .map(|line| Line::single(Role::Plain, line)),
            );
        }
        if report.source_state != ValidationSourceState::Unchanged {
            doc.notes([Line::single(Role::Dim, "Source inputs changed or could not be verified; these results do not validate the current workspace")]);
        }
        doc
    }
}
