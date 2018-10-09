#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Instruction {
    /// Load native program by name
    LoadNative { name: String },

    /// Load BPF program by name
    LoadBpfFile { name: String },

    /// Load BPF program by file bits
    LoadBpf { offset: u64, prog: Vec<u8> },

    /// Load account with state information
    LoadState { offset: u64, data: Vec<u8> },

    /// Call a program
    Call { input: Vec<u8> },
}
