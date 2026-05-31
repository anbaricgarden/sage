pub mod action_graph;
pub mod checkpoint;
pub mod editor;
pub mod executor;
pub mod orchestrator;
pub mod planner;
pub mod reviewer;

pub use action_graph::{ActionGraph, ActionNode, ActionType};
pub use checkpoint::{Checkpoint, CheckpointManager};
pub use executor::ExecutorAgent;
pub use orchestrator::{Orchestrator, OrchestratorState};
pub use planner::PlannerAgent;
pub use reviewer::{ReviewDecision, ReviewerAgent};

pub trait Agent {
    fn name(&self) -> &'static str;
}
