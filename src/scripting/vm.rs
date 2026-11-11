//! Blockly virtual machine.
//!
//! Programs are snapshotted when the VM starts. Execution uses an explicit frame
//! stack, so nested `Repeat`, `If`, `While` and action calls all resume correctly
//! across `Wait` blocks. A per-tick instruction budget and a call-depth limit
//! keep malformed scripts from freezing the editor.

use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::scripting::context::{
    ScriptCommand, ScriptEvent, ScriptOwner, SensorValues, WorldCommand,
};
use crate::scripting::library::ScriptLibrary;
use crate::scripting::context::{GameEvent, GameEventRecord};
use crate::scripting::model::{ActionDef, CompareOp, Operand, ScriptBlock, ScriptProgram};

/// Hard cap on queued world commands between two drains.
const MAX_PENDING_COMMANDS: usize = 1_024;
/// Hard cap on simultaneous program instances (bound programs).
pub const MAX_INSTANCES: usize = 512;
/// Hard cap on queued event runs per instance.
const MAX_QUEUED_EVENTS: usize = 16;

/// Maximum instructions executed by one program context in a single tick.
pub const MAX_STEPS_PER_TICK: u32 = 8_192;
/// Maximum nested action calls.
pub const MAX_CALL_DEPTH: usize = 16;
/// Upper bound applied to repeat counts and while iterations.
pub const MAX_LOOP_ITERATIONS: u32 = 10_000;
/// Hard cap on stored signals, so a runaway program cannot grow forever.
const MAX_PENDING_SIGNALS: usize = 256;
/// Hard cap on log entries retained between UI drains.
const MAX_LOG_ENTRIES: usize = 512;
/// Hard cap on the numbers of one list.
const MAX_LIST_LENGTH: usize = 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptLogLevel {
    Info,
    Action,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct ScriptLogEntry {
    pub level: ScriptLogLevel,
    pub program: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramStatus {
    Idle,
    Running,
    Waiting,
    Completed,
    Disabled,
    Error,
}

impl ProgramStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "IDLE",
            Self::Running => "RUNNING",
            Self::Waiting => "WAITING",
            Self::Completed => "COMPLETED",
            Self::Disabled => "DISABLED",
            Self::Error => "ERROR",
        }
    }
}

/// UI-facing snapshot of one running program.
#[derive(Clone, Debug)]
pub struct ProgramStatusView {
    pub name: String,
    pub status: ProgramStatus,
    pub steps: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
enum FrameKind {
    Root,
    Repeat { remaining: u32 },
    While {
        variable: String,
        op: CompareOp,
        value: Operand,
        max_iterations: u32,
    },
    Branch,
    Action { name: String },
}

#[derive(Clone, Debug)]
struct Frame {
    blocks: Vec<ScriptBlock>,
    index: usize,
    kind: FrameKind,
    iterations: u32,
}

impl Frame {
    fn root(blocks: Vec<ScriptBlock>) -> Self {
        Self {
            blocks,
            index: 0,
            kind: FrameKind::Root,
            iterations: 0,
        }
    }

    fn repeat(blocks: Vec<ScriptBlock>, times: u32) -> Self {
        Self {
            blocks,
            index: 0,
            kind: FrameKind::Repeat { remaining: times },
            iterations: 0,
        }
    }

    fn while_(
        blocks: Vec<ScriptBlock>,
        variable: String,
        op: CompareOp,
        value: Operand,
        max_iterations: u32,
    ) -> Self {
        Self {
            blocks,
            index: 0,
            kind: FrameKind::While {
                variable,
                op,
                value,
                max_iterations,
            },
            iterations: 0,
        }
    }

    fn branch(blocks: Vec<ScriptBlock>) -> Self {
        Self {
            blocks,
            index: 0,
            kind: FrameKind::Branch,
            iterations: 0,
        }
    }

    fn action(blocks: Vec<ScriptBlock>, name: String) -> Self {
        Self {
            blocks,
            index: 0,
            kind: FrameKind::Action { name },
            iterations: 0,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ExecutionContext {
    frames: Vec<Frame>,
    wait_remaining: f32,
}

impl ExecutionContext {
    fn is_idle(&self) -> bool {
        self.frames.is_empty() && self.wait_remaining <= 0.0
    }
}

#[derive(Clone, Debug)]
struct RunningProgram {
    name: String,
    enabled: bool,
    owner: ScriptOwner,
    template: ScriptProgram,
    on_start: Vec<ScriptBlock>,
    on_tick: Vec<ScriptBlock>,
    start: ExecutionContext,
    tick: ExecutionContext,
    /// Event handler runs (interact, hit, signal, energy) in flight.
    events: Vec<ExecutionContext>,
    /// Instance-local variables (bound programs). Global programs use the
    /// shared table directly.
    locals: BTreeMap<String, f32>,
    sensors: SensorValues,
    status: ProgramStatus,
    steps: u64,
    error: Option<String>,
}

impl RunningProgram {
    fn new(program: &ScriptProgram) -> Self {
        Self::with_owner(program, ScriptOwner::Global)
    }

    fn with_owner(program: &ScriptProgram, owner: ScriptOwner) -> Self {
        Self {
            name: program.name.clone(),
            enabled: program.enabled,
            owner,
            template: program.clone(),
            on_start: program.on_start.clone(),
            on_tick: program.on_tick.clone(),
            start: ExecutionContext::default(),
            tick: ExecutionContext::default(),
            events: Vec::new(),
            locals: BTreeMap::new(),
            sensors: SensorValues::default(),
            status: if program.enabled {
                ProgramStatus::Idle
            } else {
                ProgramStatus::Disabled
            },
            steps: 0,
            error: None,
        }
    }

    fn is_global(&self) -> bool {
        self.owner == ScriptOwner::Global
    }
}

/// State shared by every program in one run.
#[derive(Clone, Debug)]
pub struct VmShared {
    pub variables: BTreeMap<String, f32>,
    pub signals: Vec<String>,
    pub log: Vec<ScriptLogEntry>,
    pub instructions: u64,
    pub avatar_position: [f32; 3],
    pub avatar_yaw: f32,
    pub energy: f32,
    pub energy_capacity: f32,
    pub commands: Vec<ScriptCommand>,
    pub rng_state: u64,
    pub lists: BTreeMap<String, Vec<f32>>,
    /// `Broadcast` blocks waiting for the end of the tick.
    pub broadcasts: Vec<(String, f32)>,
    /// A `Breakpoint` was reached.
    pub paused: bool,
}

impl Default for VmShared {
    fn default() -> Self {
        Self {
            variables: BTreeMap::new(),
            signals: Vec::new(),
            log: Vec::new(),
            instructions: 0,
            avatar_position: [0.0, 10.0, 0.0],
            avatar_yaw: 0.0,
            energy: 0.0,
            energy_capacity: 100.0,
            commands: Vec::new(),
            rng_state: 0x9E37_79B9_7F4A_7C15,
            lists: BTreeMap::new(),
            broadcasts: Vec::new(),
            paused: false,
        }
    }
}

impl VmShared {
    fn variable(&self, name: &str) -> f32 {
        self.variables.get(name).copied().unwrap_or(0.0)
    }

    /// xorshift64*: deterministic per run, good enough for gameplay noise.
    fn random(&mut self) -> f32 {
        let mut x = self.rng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng_state = x;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / ((1u64 << 24) as f32)
    }

    fn push_command(&mut self, owner: ScriptOwner, program: &str, command: WorldCommand) {
        if self.commands.len() < MAX_PENDING_COMMANDS {
            self.commands.push(ScriptCommand {
                owner,
                program: program.to_string(),
                command,
            });
        }
    }

    fn push_log(&mut self, level: ScriptLogLevel, program: &str, message: impl Into<String>) {
        self.log.push(ScriptLogEntry {
            level,
            program: program.to_string(),
            message: message.into(),
        });
        if self.log.len() > MAX_LOG_ENTRIES {
            let overflow = self.log.len() - MAX_LOG_ENTRIES;
            self.log.drain(0..overflow);
        }
    }
}

/// Runtime state for the Blockly VM.
#[derive(Resource, Debug, Default)]
pub struct ScriptRuntime {
    running: bool,
    pie_owned: bool,
    last_revision: u64,
    tick_count: u64,
    programs: Vec<RunningProgram>,
    /// Bound programs, instantiated per block / entity on demand.
    templates: Vec<ScriptProgram>,
    actions: Vec<ActionDef>,
    shared: VmShared,
    last_error: Option<String>,
    step_requested: bool,
}

impl ScriptRuntime {
    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn is_pie_owned(&self) -> bool {
        self.pie_owned
    }

    pub fn last_revision(&self) -> u64 {
        self.last_revision
    }

    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    pub fn instructions(&self) -> u64 {
        self.shared.instructions
    }

    pub fn program_count(&self) -> usize {
        self.programs.len()
    }

    pub fn signal_count(&self) -> usize {
        self.shared.signals.len()
    }

    pub fn energy(&self) -> (f32, f32) {
        (self.shared.energy, self.shared.energy_capacity)
    }

    pub fn avatar(&self) -> ([f32; 3], f32) {
        (self.shared.avatar_position, self.shared.avatar_yaw)
    }

    pub fn variables(&self) -> &BTreeMap<String, f32> {
        &self.shared.variables
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn statuses(&self) -> Vec<ProgramStatusView> {
        self.programs
            .iter()
            .map(|program| ProgramStatusView {
                name: program.name.clone(),
                status: program.status,
                steps: program.steps,
                error: program.error.clone(),
            })
            .collect()
    }

    /// Drain the log entries accumulated since the previous UI frame.
    pub fn drain_log(&mut self) -> Vec<ScriptLogEntry> {
        std::mem::take(&mut self.shared.log)
    }

    /// Drain pending signals. Gameplay systems will consume these in a later
    /// stage; the console command `script signals` uses it today.
    pub fn drain_signals(&mut self) -> Vec<String> {
        std::mem::take(&mut self.shared.signals)
    }

    /// Start every enabled program.
    pub fn start(&mut self, library: &ScriptLibrary, pie_owned: bool) {
        self.running = true;
        self.pie_owned = pie_owned;
        self.last_revision = library.revision;
        self.tick_count = 0;
        self.last_error = None;
        self.shared = VmShared::default();
        self.actions = library.actions.clone();
        self.templates = library
            .programs
            .iter()
            .filter(|program| program.bound)
            .cloned()
            .collect();
        self.programs = library
            .programs
            .iter()
            .filter(|program| !program.bound)
            .map(RunningProgram::new)
            .collect();

        let mut started = 0;
        for program in &mut self.programs {
            if program.enabled && !program.on_start.is_empty() {
                program
                    .start
                    .frames
                    .push(Frame::root(program.on_start.clone()));
                program.status = ProgramStatus::Running;
                started += 1;
            }
        }

        self.shared.push_log(
            ScriptLogLevel::Action,
            "VM",
            format!(
                "Started {started} program(s) at library revision {}.",
                library.revision
            ),
        );
    }

    /// Start a single named program and disable the others for this run.
    pub fn start_named(&mut self, library: &ScriptLibrary, name: &str, pie_owned: bool) -> bool {
        self.start(library, pie_owned);

        let mut found = false;
        for program in &mut self.programs {
            if program.name == name {
                found = true;
                program.enabled = true;
                if program.start.frames.is_empty() && !program.on_start.is_empty() {
                    program
                        .start
                        .frames
                        .push(Frame::root(program.on_start.clone()));
                }
                program.status = ProgramStatus::Running;
            } else {
                program.enabled = false;
                program.start = ExecutionContext::default();
                program.tick = ExecutionContext::default();
                program.status = ProgramStatus::Disabled;
            }
        }

        if found {
            self.shared.push_log(
                ScriptLogLevel::Action,
                "VM",
                format!("Running only program '{name}'."),
            );
        } else {
            self.stop();
        }
        found
    }

    pub fn stop(&mut self) {
        self.running = false;
        self.pie_owned = false;
        self.last_revision = 0;
        self.tick_count = 0;
        self.programs.clear();
        self.templates.clear();
        self.actions.clear();
        self.shared = VmShared::default();
        self.last_error = None;
    }

    /// Names of bound programs that can be attached to blocks or entities.
    pub fn template_names(&self) -> Vec<String> {
        self.templates.iter().map(|program| program.name.clone()).collect()
    }

    /// Instantiate a program (bound template or regular program) for an owner.
    pub fn spawn_instance(&mut self, name: &str, owner: ScriptOwner) -> bool {
        if !self.running || self.programs.len() >= MAX_INSTANCES {
            return false;
        }
        if self
            .programs
            .iter()
            .any(|program| program.owner == owner && program.name == name)
        {
            return true;
        }
        let template = self
            .templates
            .iter()
            .find(|program| program.name == name)
            .cloned()
            .or_else(|| {
                self.programs
                    .iter()
                    .find(|program| program.name == name)
                    .map(|program| program.template.clone())
            });
        let Some(template) = template else {
            return false;
        };

        let mut instance = RunningProgram::with_owner(&template, owner);
        if instance.enabled && !instance.on_start.is_empty() {
            instance
                .start
                .frames
                .push(Frame::root(instance.on_start.clone()));
            instance.status = ProgramStatus::Running;
        }
        self.programs.push(instance);
        true
    }

    /// Remove every instance bound to an owner (entity died, block broken).
    pub fn despawn_owner(&mut self, owner: ScriptOwner) {
        if owner == ScriptOwner::Global {
            return;
        }
        self.programs.retain(|program| program.owner != owner);
    }

    pub fn instance_count(&self) -> usize {
        self.programs.iter().filter(|program| !program.is_global()).count()
    }

    /// Update the sensor snapshot of every instance of an owner.
    pub fn set_sensors(&mut self, owner: ScriptOwner, sensors: SensorValues) {
        for program in &mut self.programs {
            if program.owner == owner {
                program.sensors = sensors;
            }
        }
    }

    /// Update the sensors shared by every global program.
    pub fn set_global_sensors(&mut self, sensors: SensorValues) {
        self.set_sensors(ScriptOwner::Global, sensors);
    }

    /// Queue the matching handler on every instance of `owner`.
    /// Signals are also broadcast to global programs when `owner` is Global.
    pub fn fire_event(&mut self, owner: ScriptOwner, event: &ScriptEvent) -> usize {
        if !self.running {
            return 0;
        }
        let mut fired = 0;
        for program in &mut self.programs {
            if program.owner != owner || !program.enabled || program.error.is_some() {
                continue;
            }
            let (blocks, matches) = match event {
                ScriptEvent::Interact => (&program.template.on_interact, true),
                ScriptEvent::Hit { .. } => (&program.template.on_hit, true),
                ScriptEvent::Signal { port, .. } => (
                    &program.template.on_signal,
                    program.template.signal_port.is_empty()
                        || program.template.signal_port == *port,
                ),
                ScriptEvent::Energy { .. } => (&program.template.on_energy, true),
            };
            if !matches || blocks.is_empty() || program.events.len() >= MAX_QUEUED_EVENTS {
                continue;
            }
            let blocks = blocks.clone();

            let target = if program.owner == ScriptOwner::Global {
                &mut self.shared.variables
            } else {
                &mut program.locals
            };
            match event {
                ScriptEvent::Hit { damage } => {
                    target.insert("damage".to_string(), *damage);
                }
                ScriptEvent::Signal { port, value } => {
                    target.insert("signal".to_string(), *value);
                    target.insert(format!("port_{port}"), *value);
                }
                ScriptEvent::Energy { powered } => {
                    target.insert("powered".to_string(), if *powered { 1.0 } else { 0.0 });
                }
                ScriptEvent::Interact => {}
            }

            let mut context = ExecutionContext::default();
            context.frames.push(Frame::root(blocks));
            program.events.push(context);
            fired += 1;
        }
        fired
    }

    /// World commands produced since the last drain.
    pub fn drain_commands(&mut self) -> Vec<ScriptCommand> {
        std::mem::take(&mut self.shared.commands)
    }

    /// Queue the matching event handlers of every program.
    pub fn fire_game_event(&mut self, record: &GameEventRecord) -> usize {
        if !self.running {
            return 0;
        }
        let mut fired = 0;
        for program in &mut self.programs {
            if !program.enabled || program.error.is_some() {
                continue;
            }
            let handlers: Vec<Vec<ScriptBlock>> = program
                .template
                .events
                .iter()
                .filter(|handler| handler.event == record.event && (handler.filter.is_empty() || handler.filter == record.key))
                .map(|handler| handler.blocks.clone())
                .collect();
            for blocks in handlers {
                if blocks.is_empty() || program.events.len() >= MAX_QUEUED_EVENTS {
                    continue;
                }
                let target = if program.owner == ScriptOwner::Global { &mut self.shared.variables } else { &mut program.locals };
                target.insert("event_value".to_string(), record.value);
                if let Some([x, y, z]) = record.position {
                    target.insert("event_x".to_string(), x);
                    target.insert("event_y".to_string(), y);
                    target.insert("event_z".to_string(), z);
                }
                let mut context = ExecutionContext::default();
                context.frames.push(Frame::root(blocks));
                program.events.push(context);
                fired += 1;
            }
        }
        fired
    }

    /// Write the answer to an `Ask`, or any variable of an instance.
    pub fn set_variable(&mut self, owner: ScriptOwner, program: &str, name: &str, value: f32) {
        if owner == ScriptOwner::Global || name.starts_with("g.") {
            self.shared.variables.insert(name.to_string(), value);
            return;
        }
        for instance in &mut self.programs {
            if instance.owner == owner && instance.name == program {
                instance.locals.insert(name.to_string(), value);
            }
        }
    }

    pub fn is_paused(&self) -> bool {
        self.shared.paused
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.shared.paused = paused;
    }

    /// Run one tick while paused.
    pub fn step(&mut self) {
        self.step_requested = true;
    }

    /// Variables of the instances, for the debugger.
    pub fn instance_variables(&self) -> Vec<(String, BTreeMap<String, f32>)> {
        self.programs
            .iter()
            .filter(|program| !program.locals.is_empty())
            .map(|program| (format!("{} {:?}", program.name, program.owner), program.locals.clone()))
            .collect()
    }

    pub fn lists(&self) -> &BTreeMap<String, Vec<f32>> {
        &self.shared.lists
    }

    /// Advance every program by `delta_seconds`.
    pub fn tick(&mut self, delta_seconds: f32) {
        if !self.running {
            return;
        }
        if self.shared.paused && !std::mem::take(&mut self.step_requested) {
            return;
        }

        let delta = delta_seconds.clamp(0.0, 0.25);
        self.tick_count = self.tick_count.wrapping_add(1);

        let Self {
            programs,
            actions,
            shared,
            ..
        } = self;

        for program in programs.iter_mut() {
            if !program.enabled {
                program.status = ProgramStatus::Disabled;
                continue;
            }
            if program.error.is_some() {
                program.status = ProgramStatus::Error;
                continue;
            }

            let global = program.is_global();
            let owner = program.owner;
            let sensors = program.sensors;

            if !program.start.is_idle() {
                let before = shared.instructions;
                let mut env = Env {
                    owner,
                    global,
                    locals: &mut program.locals,
                    sensors,
                };
                advance_context(
                    &mut program.start,
                    actions,
                    shared,
                    &mut env,
                    &program.name,
                    &mut program.error,
                    delta,
                );
                program.steps += shared.instructions - before;
            }

            if program.tick.is_idle() && !program.on_tick.is_empty() && program.error.is_none() {
                program.tick.frames.push(Frame::root(program.on_tick.clone()));
            }
            if !program.tick.is_idle() {
                let before = shared.instructions;
                let mut env = Env {
                    owner,
                    global,
                    locals: &mut program.locals,
                    sensors,
                };
                advance_context(
                    &mut program.tick,
                    actions,
                    shared,
                    &mut env,
                    &program.name,
                    &mut program.error,
                    delta,
                );
                program.steps += shared.instructions - before;
            }

            for context in &mut program.events {
                if program.error.is_some() {
                    break;
                }
                let before = shared.instructions;
                let mut env = Env {
                    owner,
                    global,
                    locals: &mut program.locals,
                    sensors,
                };
                advance_context(
                    context,
                    actions,
                    shared,
                    &mut env,
                    &program.name,
                    &mut program.error,
                    delta,
                );
                program.steps += shared.instructions - before;
            }
            program.events.retain(|context| !context.is_idle());

            program.status = if program.error.is_some() {
                ProgramStatus::Error
            } else if program.start.wait_remaining > 0.0 || program.tick.wait_remaining > 0.0 {
                ProgramStatus::Waiting
            } else if !program.start.is_idle() || !program.tick.is_idle() {
                ProgramStatus::Running
            } else if program.on_tick.is_empty() {
                ProgramStatus::Completed
            } else {
                ProgramStatus::Running
            };
        }

        self.last_error = self
            .programs
            .iter()
            .find_map(|program| program.error.clone());

        for (message, value) in std::mem::take(&mut self.shared.broadcasts) {
            self.fire_game_event(&GameEventRecord { event: GameEvent::Message, key: message, value, position: None });
        }
    }
}

/// Per-instance view used while executing a context.
struct Env<'a> {
    owner: ScriptOwner,
    global: bool,
    locals: &'a mut BTreeMap<String, f32>,
    sensors: SensorValues,
}

impl Env<'_> {
    /// Locals shadow shared variables; names prefixed with `g.` are always shared.
    fn read(&self, shared: &VmShared, name: &str) -> f32 {
        if !self.global && !name.starts_with("g.") {
            if let Some(value) = self.locals.get(name) {
                return *value;
            }
        }
        shared.variable(name)
    }

    fn value(&self, shared: &VmShared, operand: &Operand) -> f32 {
        match operand {
            Operand::Number(value) => *value,
            Operand::Var(name) => self.read(shared, name),
        }
    }

    /// A list is private to its owner unless its name starts with `g.`.
    fn list_key(&self, name: &str) -> String {
        if self.global || name.starts_with("g.") {
            name.to_string()
        } else {
            format!("{:?}/{name}", self.owner)
        }
    }

    /// `{name}` is replaced by the variable.
    fn format(&self, shared: &VmShared, text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else {
                break;
            };
            out.push_str(&rest[..open]);
            let name = &rest[open + 1..open + close];
            let value = self.read(shared, name);
            if value.fract() == 0.0 {
                out.push_str(&format!("{}", value as i64));
            } else {
                out.push_str(&format!("{value:.2}"));
            }
            rest = &rest[open + close + 1..];
        }
        out.push_str(rest);
        out
    }

    fn write(&mut self, shared: &mut VmShared, name: String, value: f32) {
        if self.global || name.starts_with("g.") {
            shared.variables.insert(name, value);
        } else {
            self.locals.insert(name, value);
        }
    }
}

fn advance_context(
    context: &mut ExecutionContext,
    actions: &[ActionDef],
    shared: &mut VmShared,
    env: &mut Env<'_>,
    program: &str,
    error: &mut Option<String>,
    delta: f32,
) {
    if context.wait_remaining > 0.0 {
        context.wait_remaining -= delta;
        if context.wait_remaining > 0.0 {
            return;
        }
        context.wait_remaining = 0.0;
    }

    let mut steps = 0u32;
    while !context.frames.is_empty() {
        if steps >= MAX_STEPS_PER_TICK {
            let message = format!(
                "Instruction budget of {MAX_STEPS_PER_TICK} reached; the program was halted for this run."
            );
            shared.push_log(ScriptLogLevel::Warn, program, message.clone());
            if error.is_none() {
                *error = Some(message);
            }
            return;
        }
        steps += 1;
        shared.instructions = shared.instructions.wrapping_add(1);

        let top = context.frames.len() - 1;
        let finished = context.frames[top].index >= context.frames[top].blocks.len();
        if finished {
            let loop_again = {
                let frame = &context.frames[top];
                match &frame.kind {
                    FrameKind::Repeat { remaining } => *remaining > 1,
                    FrameKind::While {
                        variable,
                        op,
                        value,
                        max_iterations,
                    } => {
                        frame.iterations < *max_iterations
                            && op.evaluate(env.read(shared, variable), env.value(shared, value))
                    }
                    _ => false,
                }
            };

            if loop_again {
                let frame = &mut context.frames[top];
                match &mut frame.kind {
                    FrameKind::Repeat { remaining } => {
                        *remaining -= 1;
                        frame.index = 0;
                    }
                    FrameKind::While { .. } => {
                        frame.index = 0;
                        frame.iterations += 1;
                    }
                    _ => {}
                }
                continue;
            }

            if let FrameKind::Action { name } = &context.frames[top].kind {
                let name = name.clone();
                shared.push_log(
                    ScriptLogLevel::Info,
                    program,
                    format!("Action '{name}' completed."),
                );
            }
            context.frames.pop();
            continue;
        }

        let block = context.frames[top].blocks[context.frames[top].index].clone();
        context.frames[top].index += 1;

        match block {
            ScriptBlock::Wait { seconds } => {
                let seconds = env.value(shared, &seconds).max(0.0);
                if seconds > 0.0 {
                    context.wait_remaining = seconds;
                    return;
                }
            }
            ScriptBlock::Move { distance } => {
                let yaw = shared.avatar_yaw;
                shared.avatar_position[0] += yaw.sin() * distance;
                shared.avatar_position[2] += yaw.cos() * distance;
            }
            ScriptBlock::Rotate { degrees } => {
                shared.avatar_yaw =
                    (shared.avatar_yaw + degrees.to_radians()) % std::f32::consts::TAU;
            }
            ScriptBlock::Emit { signal } => {
                if shared.signals.len() < MAX_PENDING_SIGNALS {
                    shared.signals.push(signal.clone());
                }
                shared.push_log(
                    ScriptLogLevel::Action,
                    program,
                    format!("Emit '{signal}'."),
                );
            }
            ScriptBlock::Log { message } => {
                shared.push_log(ScriptLogLevel::Info, program, message);
            }
            ScriptBlock::SetVar { name, value } => {
                let value = env.value(shared, &value);
                env.write(shared, name, value);
            }
            ScriptBlock::AddVar { name, value } => {
                let current = env.read(shared, &name);
                let value = env.value(shared, &value);
                env.write(shared, name, current + value);
            }
            ScriptBlock::Math { name, op, value } => {
                let current = env.read(shared, &name);
                let value = env.value(shared, &value);
                env.write(shared, name, op.apply(current, value));
            }
            ScriptBlock::Compute { name, left, op, right } => {
                let result = op.apply(env.value(shared, &left), env.value(shared, &right));
                env.write(shared, name, result);
            }
            ScriptBlock::RandomRange { name, min, max } => {
                let (min, max) = (env.value(shared, &min), env.value(shared, &max));
                let result = min + (max - min) * shared.random();
                env.write(shared, name, result);
            }
            ScriptBlock::Ask { name, question } => {
                shared.push_command(env.owner, program, WorldCommand::Ask { name, question });
                context.wait_remaining = f32::MIN_POSITIVE;
                return;
            }
            ScriptBlock::ListPush { list, value } => {
                let value = env.value(shared, &value);
                let entry = shared.lists.entry(env.list_key(&list)).or_default();
                if entry.len() < MAX_LIST_LENGTH {
                    entry.push(value);
                }
            }
            ScriptBlock::ListGet { name, list, index } => {
                let index = env.value(shared, &index).max(0.0) as usize;
                let value = shared.lists.get(&env.list_key(&list)).and_then(|entry| entry.get(index)).copied().unwrap_or(0.0);
                env.write(shared, name, value);
            }
            ScriptBlock::ListSet { list, index, value } => {
                let index = env.value(shared, &index).max(0.0) as usize;
                let value = env.value(shared, &value);
                if let Some(slot) = shared.lists.get_mut(&env.list_key(&list)).and_then(|entry| entry.get_mut(index)) {
                    *slot = value;
                }
            }
            ScriptBlock::ListLength { name, list } => {
                let length = shared.lists.get(&env.list_key(&list)).map_or(0, Vec::len);
                env.write(shared, name, length as f32);
            }
            ScriptBlock::ListClear { list } => {
                shared.lists.remove(&env.list_key(&list));
            }
            ScriptBlock::Broadcast { message, value } => {
                let value = env.value(shared, &value);
                if shared.broadcasts.len() < MAX_PENDING_SIGNALS {
                    shared.broadcasts.push((message, value));
                }
            }
            ScriptBlock::GiveItem { key, amount } => {
                let amount = env.value(shared, &amount).max(0.0) as u32;
                shared.push_command(env.owner, program, WorldCommand::GiveItem { key, amount });
            }
            ScriptBlock::TakeItem { key, amount, result } => {
                let amount = env.value(shared, &amount).max(0.0) as u32;
                shared.push_command(env.owner, program, WorldCommand::TakeItem { key, amount, result });
                context.wait_remaining = f32::MIN_POSITIVE;
                return;
            }
            ScriptBlock::SetPlayerStat { stat, value, add } => {
                let value = env.value(shared, &value);
                shared.push_command(env.owner, program, WorldCommand::SetPlayerStat { stat, value, add });
            }
            ScriptBlock::DamagePlayer { amount } => {
                let amount = env.value(shared, &amount);
                shared.push_command(env.owner, program, WorldCommand::DamagePlayer { amount });
            }
            ScriptBlock::TeleportPlayer { x, y, z } => {
                let position = [env.value(shared, &x), env.value(shared, &y), env.value(shared, &z)];
                shared.push_command(env.owner, program, WorldCommand::TeleportPlayer { position });
            }
            ScriptBlock::Shoot { damage, range } => {
                let (damage, range) = (env.value(shared, &damage), env.value(shared, &range));
                shared.push_command(env.owner, program, WorldCommand::Shoot { damage, range });
            }
            ScriptBlock::MoveTo { x, y, z, speed } => {
                let position = [env.value(shared, &x), env.value(shared, &y), env.value(shared, &z)];
                shared.push_command(env.owner, program, WorldCommand::MoveTo { position, speed });
            }
            ScriptBlock::ShowMessage { text } => {
                let text = env.format(shared, &text);
                shared.push_command(env.owner, program, WorldCommand::ShowMessage { text });
            }
            ScriptBlock::SetWeather { weather } => {
                shared.push_command(env.owner, program, WorldCommand::SetWeather { weather });
            }
            ScriptBlock::StartQuest { quest } => {
                shared.push_command(env.owner, program, WorldCommand::StartQuest { quest });
            }
            ScriptBlock::ShowDialogue { dialogue } => {
                shared.push_command(env.owner, program, WorldCommand::ShowDialogue { dialogue });
            }
            ScriptBlock::OpenScreen { screen } => {
                shared.push_command(env.owner, program, WorldCommand::OpenScreen { screen });
            }
            ScriptBlock::CloseScreen { screen } => {
                shared.push_command(env.owner, program, WorldCommand::CloseScreen { screen });
            }
            ScriptBlock::SaveValue { key, value } => {
                let value = env.value(shared, &value);
                shared.push_command(env.owner, program, WorldCommand::SaveValue { key, value });
            }
            ScriptBlock::Breakpoint => {
                shared.paused = true;
                shared.push_log(ScriptLogLevel::Action, program, "Breakpoint reached.");
                return;
            }
            ScriptBlock::Sense { name, sensor } => {
                let random = shared.random();
                let value = env.sensors.read(sensor, random);
                env.write(shared, name, value);
            }
            ScriptBlock::SetBlock { offset, block, rot } => {
                shared.push_command(env.owner, program, WorldCommand::SetBlock { offset, block, rot });
            }
            ScriptBlock::BreakBlock { offset, drop } => {
                shared.push_command(env.owner, program, WorldCommand::BreakBlock { offset, drop });
            }
            ScriptBlock::SpawnParticle { kind, count } => {
                let count = count.min(256);
                shared.push_command(env.owner, program, WorldCommand::SpawnParticle { kind, count });
            }
            ScriptBlock::DropResource { resource, amount } => {
                shared.push_command(env.owner, program, WorldCommand::DropResource { resource, amount });
            }
            ScriptBlock::PlaySound { sound } => {
                shared.push_command(env.owner, program, WorldCommand::PlaySound { sound });
            }
            ScriptBlock::SetTimeOfDay { value } => {
                // A negative literal means "use the `time` variable".
                let value = if value < 0.0 { env.read(shared, "time") } else { value };
                shared.push_command(env.owner, program, WorldCommand::SetTimeOfDay { value });
            }
            ScriptBlock::SpawnMob { mob } => {
                shared.push_command(env.owner, program, WorldCommand::SpawnMob { mob });
            }
            ScriptBlock::MoveToward { target, speed } => {
                shared.push_command(env.owner, program, WorldCommand::MoveToward { target, speed });
            }
            ScriptBlock::ApplyImpulse { impulse } => {
                shared.push_command(env.owner, program, WorldCommand::ApplyImpulse { impulse });
            }
            ScriptBlock::PlayAction { action, blend } => {
                shared.push_command(env.owner, program, WorldCommand::PlayAction { action, blend });
            }
            ScriptBlock::SetHealth { amount } => {
                let amount = env.value(shared, &amount);
                shared.push_command(env.owner, program, WorldCommand::SetHealth { amount });
            }
            ScriptBlock::Attack { damage, range } => {
                shared.push_command(env.owner, program, WorldCommand::Attack { damage, range });
            }
            ScriptBlock::TameEntity => {
                shared.push_command(env.owner, program, WorldCommand::TameEntity);
            }
            ScriptBlock::SetSignal { port, value } => {
                let value = env.value(shared, &value);
                shared.push_command(env.owner, program, WorldCommand::SetSignal { port, value });
            }
            ScriptBlock::AddEnergy { amount } => {
                shared.energy = (shared.energy + amount).clamp(0.0, shared.energy_capacity);
            }
            ScriptBlock::Repeat { times, body } => {
                let times = times.min(MAX_LOOP_ITERATIONS);
                if times > 0 && !body.is_empty() {
                    context.frames.push(Frame::repeat(body, times));
                }
            }
            ScriptBlock::While {
                variable,
                op,
                value,
                max_iterations,
                body,
            } => {
                let max_iterations = max_iterations.clamp(1, MAX_LOOP_ITERATIONS);
                if !body.is_empty() && op.evaluate(env.read(shared, &variable), env.value(shared, &value)) {
                    context.frames.push(Frame::while_(
                        body,
                        variable,
                        op,
                        value,
                        max_iterations,
                    ));
                }
            }
            ScriptBlock::If {
                variable,
                op,
                value,
                then_body,
                else_body,
            } => {
                let body = if op.evaluate(env.read(shared, &variable), env.value(shared, &value)) {
                    then_body
                } else {
                    else_body
                };
                if !body.is_empty() {
                    context.frames.push(Frame::branch(body));
                }
            }
            ScriptBlock::CallAction { action, args } => {
                for (name, value) in args {
                    let value = env.value(shared, &value);
                    env.write(shared, name, value);
                }
                let depth = context
                    .frames
                    .iter()
                    .filter(|frame| matches!(frame.kind, FrameKind::Action { .. }))
                    .count();
                if depth >= MAX_CALL_DEPTH {
                    let message =
                        format!("Action call depth limit ({MAX_CALL_DEPTH}) reached for '{action}'.");
                    shared.push_log(ScriptLogLevel::Error, program, message.clone());
                    if error.is_none() {
                        *error = Some(message);
                    }
                    continue;
                }

                match actions.iter().find(|definition| definition.name == action) {
                    Some(definition) => {
                        context
                            .frames
                            .push(Frame::action(definition.blocks.clone(), action));
                    }
                    None => {
                        let message = format!("Unknown action '{action}'.");
                        shared.push_log(ScriptLogLevel::Error, program, message.clone());
                        if error.is_none() {
                            *error = Some(message);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::library::ScriptLibrary;
    use crate::scripting::model::ActionDef;

    fn program(name: &str, on_start: Vec<ScriptBlock>, on_tick: Vec<ScriptBlock>) -> ScriptProgram {
        ScriptProgram {
            name: name.to_string(),
            enabled: true,
            description: String::new(),
            on_start,
            on_tick,
            ..ScriptProgram::new(String::new())
        }
    }

    fn library(programs: Vec<ScriptProgram>, actions: Vec<ActionDef>) -> ScriptLibrary {
        let mut library = ScriptLibrary::default();
        library.programs = programs;
        library.actions = actions;
        library.revision = 1;
        library
    }

    #[test]
    fn bound_instances_keep_local_variables_and_emit_commands() {
        let mut template = ScriptProgram::new("Brain");
        template.bound = true;
        template.on_start = vec![ScriptBlock::SetVar {
            name: "n".to_string(),
            value: 2.0.into(),
        }];
        template.on_interact = vec![ScriptBlock::SetSignal {
            port: "out".to_string(),
            value: 1.0.into(),
        }];
        let library = library(vec![template], Vec::new());

        let mut runtime = ScriptRuntime::default();
        runtime.start(&library, true);
        assert!(runtime.spawn_instance("Brain", ScriptOwner::Block([1, 2, 3])));
        runtime.tick(0.016);
        assert!(runtime.variables().get("n").is_none());

        assert_eq!(runtime.fire_event(ScriptOwner::Block([1, 2, 3]), &ScriptEvent::Interact), 1);
        runtime.tick(0.016);
        let commands = runtime.drain_commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].owner, ScriptOwner::Block([1, 2, 3]));

        runtime.despawn_owner(ScriptOwner::Block([1, 2, 3]));
        assert_eq!(runtime.instance_count(), 0);
    }

    #[test]
    fn repeat_emits_in_order() {
        let library = library(
            vec![program(
                "repeat",
                vec![
                    ScriptBlock::Emit {
                        signal: "a".to_string(),
                    },
                    ScriptBlock::Repeat {
                        times: 3,
                        body: vec![ScriptBlock::Emit {
                            signal: "b".to_string(),
                        }],
                    },
                ],
                Vec::new(),
            )],
            Vec::new(),
        );

        let mut runtime = ScriptRuntime::default();
        runtime.start(&library, false);
        runtime.tick(0.016);

        assert_eq!(runtime.drain_signals(), vec!["a", "b", "b", "b"]);
        assert_eq!(runtime.statuses()[0].status, ProgramStatus::Completed);
    }

    #[test]
    fn wait_pauses_and_resumes() {
        let library = library(
            vec![program(
                "wait",
                vec![
                    ScriptBlock::Emit {
                        signal: "a".to_string(),
                    },
                    ScriptBlock::Wait { seconds: 1.0.into() },
                    ScriptBlock::Emit {
                        signal: "b".to_string(),
                    },
                ],
                Vec::new(),
            )],
            Vec::new(),
        );

        let mut runtime = ScriptRuntime::default();
        runtime.start(&library, false);
        runtime.tick(0.1);
        assert_eq!(runtime.drain_signals(), vec!["a"]);
        assert_eq!(runtime.statuses()[0].status, ProgramStatus::Waiting);

        runtime.tick(0.25);
        runtime.tick(0.25);
        runtime.tick(0.25);
        assert!(runtime.drain_signals().is_empty());

        runtime.tick(0.25);
        assert_eq!(runtime.drain_signals(), vec!["b"]);
    }

    #[test]
    fn tick_handler_runs_every_tick() {
        let library = library(
            vec![program(
                "tick",
                Vec::new(),
                vec![ScriptBlock::AddVar {
                    name: "n".to_string(),
                    value: 1.0.into(),
                }],
            )],
            Vec::new(),
        );

        let mut runtime = ScriptRuntime::default();
        runtime.start(&library, false);
        runtime.tick(0.016);
        runtime.tick(0.016);
        runtime.tick(0.016);

        assert_eq!(runtime.variables().get("n"), Some(&3.0));
    }

    #[test]
    fn if_and_while_share_variables() {
        let library = library(
            vec![program(
                "control",
                vec![
                    ScriptBlock::SetVar {
                        name: "n".to_string(),
                        value: 0.0.into(),
                    },
                    ScriptBlock::While {
                        variable: "n".to_string(),
                        op: CompareOp::Lt,
                        value: 3.0.into(),
                        max_iterations: 10,
                        body: vec![
                            ScriptBlock::AddVar {
                                name: "n".to_string(),
                                value: 1.0.into(),
                            },
                            ScriptBlock::Emit {
                                signal: "loop".to_string(),
                            },
                        ],
                    },
                    ScriptBlock::If {
                        variable: "n".to_string(),
                        op: CompareOp::Eq,
                        value: 3.0.into(),
                        then_body: vec![ScriptBlock::Emit {
                            signal: "done".to_string(),
                        }],
                        else_body: vec![ScriptBlock::Emit {
                            signal: "fail".to_string(),
                        }],
                    },
                ],
                Vec::new(),
            )],
            Vec::new(),
        );

        let mut runtime = ScriptRuntime::default();
        runtime.start(&library, false);
        runtime.tick(0.016);

        assert_eq!(runtime.drain_signals(), vec!["loop", "loop", "loop", "done"]);
        assert_eq!(runtime.variables().get("n"), Some(&3.0));
    }

    #[test]
    fn action_calls_are_reusable() {
        let library = library(
            vec![program(
                "caller",
                vec![
                    ScriptBlock::call_action_default("Step"),
                    ScriptBlock::call_action_default("Step"),
                ],
                Vec::new(),
            )],
            vec![ActionDef {
                name: "Step".to_string(),
                description: String::new(),
                blocks: vec![
                    ScriptBlock::Emit {
                        signal: "step".to_string(),
                    },
                    ScriptBlock::AddVar {
                        name: "x".to_string(),
                        value: 1.0.into(),
                    },
                ],
            }],
        );

        let mut runtime = ScriptRuntime::default();
        runtime.start(&library, false);
        runtime.tick(0.016);

        assert_eq!(runtime.drain_signals(), vec!["step", "step"]);
        assert_eq!(runtime.variables().get("x"), Some(&2.0));
    }

    #[test]
    fn start_named_disables_other_programs() {
        let library = library(
            vec![
                program(
                    "first",
                    vec![ScriptBlock::Emit {
                        signal: "first".to_string(),
                    }],
                    Vec::new(),
                ),
                program(
                    "second",
                    vec![ScriptBlock::Emit {
                        signal: "second".to_string(),
                    }],
                    Vec::new(),
                ),
            ],
            Vec::new(),
        );

        let mut runtime = ScriptRuntime::default();
        assert!(runtime.start_named(&library, "second", false));
        runtime.tick(0.016);

        assert_eq!(runtime.drain_signals(), vec!["second"]);
        let statuses = runtime.statuses();
        assert_eq!(statuses[0].status, ProgramStatus::Disabled);
        assert_eq!(statuses[1].status, ProgramStatus::Completed);
    }

    #[test]
    fn runaway_while_hits_the_budget() {
        let library = library(
            vec![program(
                "runaway",
                vec![ScriptBlock::While {
                    variable: "n".to_string(),
                    op: CompareOp::Lt,
                    value: 1_000_000.0.into(),
                    max_iterations: 10_000,
                    body: vec![ScriptBlock::AddVar {
                        name: "n".to_string(),
                        value: 1.0.into(),
                    }],
                }],
                Vec::new(),
            )],
            Vec::new(),
        );

        let mut runtime = ScriptRuntime::default();
        runtime.start(&library, false);
        runtime.tick(0.016);

        assert!(runtime.last_error().is_some());
        assert_eq!(runtime.statuses()[0].status, ProgramStatus::Error);
    }

    #[test]
    fn stop_clears_runtime_state() {
        let library = library(
            vec![program(
                "one",
                vec![ScriptBlock::Emit {
                    signal: "one".to_string(),
                }],
                Vec::new(),
            )],
            Vec::new(),
        );

        let mut runtime = ScriptRuntime::default();
        runtime.start(&library, false);
        runtime.tick(0.016);
        assert!(runtime.is_running());

        runtime.stop();
        assert!(!runtime.is_running());
        assert!(runtime.variables().is_empty());
        assert_eq!(runtime.program_count(), 0);
    }
}

#[cfg(test)]
mod game_event_tests {
    use super::*;
    use crate::scripting::context::{GameEvent, GameEventRecord, Question, SensorValues, WorldCommand};
    use crate::scripting::library::{program_templates, ScriptLibrary};
    use crate::scripting::model::{EventHandler, MathOp, Operand, ScriptProgram, ScriptTarget};

    fn run(program: ScriptProgram) -> ScriptRuntime {
        let mut library = ScriptLibrary::default();
        library.programs = vec![program];
        library.actions.clear();
        let mut runtime = ScriptRuntime::default();
        runtime.start(&library, false);
        runtime
    }

    #[test]
    fn operands_keep_old_files_readable() {
        let old: ScriptBlock = ron::from_str("SetVar(name: \"a\", value: 2.5)").expect("old number");
        assert_eq!(old, ScriptBlock::SetVar { name: "a".into(), value: Operand::Number(2.5) });
        let new = ron::to_string(&ScriptBlock::SetVar { name: "a".into(), value: Operand::Var("b".into()) }).unwrap();
        let back: ScriptBlock = ron::from_str(&new).expect("variable");
        assert!(matches!(back, ScriptBlock::SetVar { value: Operand::Var(_), .. }));
    }

    #[test]
    fn compute_and_lists_work_on_variables() {
        let mut program = ScriptProgram::new("t".to_string());
        program.enabled = true;
        program.on_start = vec![
            ScriptBlock::SetVar { name: "a".into(), value: 3.0.into() },
            ScriptBlock::Compute { name: "b".into(), left: Operand::Var("a".into()), op: MathOp::Mul, right: 4.0.into() },
            ScriptBlock::ListPush { list: "l".into(), value: Operand::Var("b".into()) },
            ScriptBlock::ListGet { name: "c".into(), list: "l".into(), index: 0.0.into() },
            ScriptBlock::ListLength { name: "d".into(), list: "l".into() },
        ];
        let mut runtime = run(program);
        runtime.tick(0.016);
        assert_eq!(runtime.variables().get("c"), Some(&12.0));
        assert_eq!(runtime.variables().get("d"), Some(&1.0));
    }

    #[test]
    fn game_events_reach_filtered_handlers_only() {
        let mut program = ScriptProgram::new("t".to_string());
        program.enabled = true;
        program.events = vec![EventHandler {
            event: GameEvent::MobKilled,
            filter: "Arana".into(),
            blocks: vec![ScriptBlock::SetVar { name: "got".into(), value: Operand::Var("event_value".into()) }],
        }];
        let mut runtime = run(program);
        let record = |key: &str| GameEventRecord { event: GameEvent::MobKilled, key: key.into(), value: 7.0, position: None };
        assert_eq!(runtime.fire_game_event(&record("Other")), 0);
        assert_eq!(runtime.fire_game_event(&record("Arana")), 1);
        runtime.tick(0.016);
        assert_eq!(runtime.variables().get("got"), Some(&7.0));
    }

    #[test]
    fn broadcast_reaches_listeners_and_ask_waits_for_the_answer() {
        let mut program = ScriptProgram::new("t".to_string());
        program.enabled = true;
        program.on_start = vec![
            ScriptBlock::Ask { name: "n".into(), question: Question::Saved { key: "k".into() } },
            ScriptBlock::SetVar { name: "after".into(), value: Operand::Var("n".into()) },
            ScriptBlock::Broadcast { message: "go".into(), value: 2.0.into() },
        ];
        program.events = vec![EventHandler {
            event: GameEvent::Message,
            filter: "go".into(),
            blocks: vec![ScriptBlock::AddVar { name: "heard".into(), value: 1.0.into() }],
        }];
        let mut runtime = run(program);
        runtime.tick(0.016);
        assert!(runtime.variables().get("after").is_none(), "must wait for the answer");
        let commands = runtime.drain_commands();
        assert!(matches!(commands[0].command, WorldCommand::Ask { .. }));
        runtime.set_variable(commands[0].owner, &commands[0].program, "n", 5.0);
        runtime.tick(0.016);
        runtime.tick(0.016);
        assert_eq!(runtime.variables().get("after"), Some(&5.0));
        assert_eq!(runtime.variables().get("heard"), Some(&1.0));
    }

    #[test]
    fn breakpoint_pauses_and_step_runs_one_tick() {
        let mut program = ScriptProgram::new("t".to_string());
        program.enabled = true;
        program.on_start = vec![
            ScriptBlock::Breakpoint,
            ScriptBlock::SetVar { name: "after".into(), value: 1.0.into() },
        ];
        let mut runtime = run(program);
        runtime.tick(0.016);
        assert!(runtime.is_paused());
        runtime.tick(0.016);
        assert!(runtime.variables().get("after").is_none());
        runtime.step();
        runtime.tick(0.016);
        assert_eq!(runtime.variables().get("after"), Some(&1.0));
    }

    #[test]
    fn the_shooter_template_keeps_its_distance_and_fires() {
        let template = program_templates().into_iter().find(|t| t.name == "Shooter").expect("template");
        let mut library = ScriptLibrary::default();
        library.programs = vec![template];
        library.actions.clear();
        assert!(library.validate().iter().all(|issue| issue.severity != crate::scripting::model::IssueSeverity::Error));
        let mut runtime = ScriptRuntime::default();
        runtime.start(&library, false);
        let owner = ScriptOwner::Entity(1);
        assert!(runtime.spawn_instance("Shooter", owner));
        runtime.set_sensors(owner, SensorValues { distance_to_player: 4.0, ..Default::default() });
        runtime.tick(0.016);
        let commands = runtime.drain_commands();
        assert!(commands.iter().any(|c| matches!(c.command, WorldCommand::MoveToward { target: ScriptTarget::AwayFromPlayer, .. })));
        assert!(commands.iter().any(|c| matches!(c.command, WorldCommand::Shoot { .. })));
    }
}
