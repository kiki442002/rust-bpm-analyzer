use serde::{Deserialize, Serialize};

/// The multicast address used for communication
pub const MULTICAST_ADDR: &str = "239.255.42.42";
/// The port used for communication
pub const MULTICAST_PORT: u16 = 44200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// Discovery request: "Who is on the network?"
    /// Usually sent by the Desktop app when starting.
    Discovery,

    /// Presence announcement: "I am here" or "I am leaving"
    /// Sent by the Embedded device periodically or in response to Discovery.
    Presence {
        /// Unique identifier of the device
        id: String,
        /// Human readable name
        name: String,
        /// True = Online, False = Going offline (Cleanup)
        online: bool,
    },

    /// Instant energy level update (0.0 to 1.0)
    /// Sent by Embedded -> Desktop
    /// High frequency, no feedback required.
    EnergyLevel { id: String, level: f32 },

    /// Command to enable/disable Auto-Gain
    /// Can be sent by Desktop -> Embedded
    SetAutoGain(bool),

    /// Feedback/State update for Auto-Gain
    /// Sent by Embedded -> Desktop when state changes (either by command or internal logic)
    /// This serves as "Feedback" that the command was taken into account.
    AutoGainState(bool),

    /// Command to enable/disable Audio Analysis
    /// Can be sent by Desktop -> Embedded
    SetAnalysis(bool),

    /// Feedback/State update for Analysis
    /// Sent by Embedded -> Desktop when state changes
    /// This serves as "Feedback" that the command was taken into account.
    AnalysisState(bool),
}
