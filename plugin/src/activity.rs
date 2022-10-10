#[derive(Clone, Copy, Debug)]
#[repr(u32)]
pub enum ResultKind {
    FileLocked = 100,
    BuildLogLine = 101,
    UntrustedPath = 102,
    CorruptedPath = 103,
    SetPhase = 104,
    Progress = 105,
    SetExpected = 106,
    PostBuildLogLine = 107,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ActivityId(pub u64);

#[derive(Clone, Copy, Debug)]
#[repr(u32)]
pub enum ActivityKind {
    Unknown = 0,
    CopyPath = 100,
    FileTransfer = 101,
    Realise = 102,
    CopyPaths = 103,
    Builds = 104,
    Build = 105,
    OptimiseStore = 106,
    VerifyPaths = 107,
    Substitute = 108,
    QueryPathInfo = 109,
    PostBuildHook = 110,
    BuildWaiting = 111,
}

#[derive(Debug)]
pub struct ActivityRecord {
    pub id: ActivityId,
    pub name: String,
    pub kind: ActivityKind,
}
