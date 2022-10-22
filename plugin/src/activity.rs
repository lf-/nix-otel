use std::fmt::Display;
use std::os::raw::c_char;
use std::{slice, str};

#[derive(Clone, Copy, Debug)]
#[repr(u32)]
pub enum ResultKind {
    Unknown = 0,
    FileLinked = 100,
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
    pub parent: Option<ActivityId>,
    pub name: String,
    pub kind: ActivityKind,
}

#[repr(C)]
pub struct FfiString {
    pub start: *const c_char,
    pub len: usize,
}

#[repr(C, u8)]
pub enum FfiField {
    String(FfiString),
    Num(u64),
}

#[derive(Debug)]
pub enum Field {
    String(String),
    Num(u64),
}

impl Display for Field {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Field::String(s) => f.write_str(&s),
            Field::Num(n) => n.fmt(f),
        }
    }
}

pub unsafe fn unmarshal_field(field: &FfiField) -> Option<Field> {
    match field {
        FfiField::String(s) => {
            let bytes = unsafe { slice::from_raw_parts(s.start as *const u8, s.len) };
            Some(Field::String(String::from(str::from_utf8(bytes).ok()?)))
        }
        FfiField::Num(n) => Some(Field::Num(*n)),
    }
}

pub unsafe fn unmarshal_fields(fields: FfiFields) -> Vec<Field> {
    let slice = unsafe { slice::from_raw_parts(fields.start, fields.count) };
    slice
        .iter()
        .filter_map(|ff| unsafe { unmarshal_field(ff) })
        .collect()
}

#[repr(C)]
pub struct FfiFields {
    pub start: *const FfiField,
    pub count: usize,
}
