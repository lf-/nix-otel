use std::{borrow::Cow, collections::HashMap, ffi::CStr, os::raw::c_char, sync::Mutex};

use opentelemetry::{
    trace::{Span as SpanT, Tracer as TracerT},
    KeyValue,
};
use opentelemetry_otlp::{ExportConfig, WithExportConfig};
use opentelemetry_sdk::{
    trace::{Span, Tracer},
    Resource,
};
use tokio::runtime::Runtime;

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

struct ActivityData {
    // FIXME: this direct use is probably abuse of otel sdk vs api distinction
    span: Span,
}

struct SpanMap(pub HashMap<ActivityId, ActivityData>);

impl SpanMap {
    fn begin(&mut self, tracer: &Tracer, act: ActivityId, name: &str) {
        let ad = ActivityData {
            span: tracer.start(Cow::Owned(name.into())),
        };

        self.0.insert(act, ad);
    }

    fn end(&mut self, act: ActivityId) {
        if let Some(ad) = self.0.get_mut(&act) {
            ad.span.end();
            self.0.remove(&act);
        }
    }
}
impl Default for SpanMap {
    fn default() -> Self {
        Self(Default::default())
    }
}

pub struct Context {
    tracer: Tracer,
    runtime: Runtime,
    active_spans: Mutex<SpanMap>,
}

#[no_mangle]
pub extern "C" fn start_activity(
    // FIXME: this lifetime is *basically* true but its also kinda evil
    cx: &'static Context,
    act: ActivityId,
    ty: ActivityKind,
    name: *const c_char,
    parent: ActivityId,
) {
    let name = unsafe { CStr::from_ptr(name as *const _) };
    let name_ = name.to_str().unwrap().to_owned();
    let tracer = cx.tracer.clone();
    println!("Start activity {act:?} {ty:?} {name:?} {parent:?}");
    cx.runtime.spawn(async move {
        let mut map = cx.active_spans.lock().unwrap();
        map.begin(&tracer, act, &name_);
    });
}

#[no_mangle]
pub extern "C" fn end_activity(cx: &Context, act: ActivityId) {
    println!("End activity {act:?}");
    let mut map = cx.active_spans.lock().unwrap();
    map.end(act);
}

type Error = Box<dyn std::error::Error>;

fn make_context() -> Result<Context, Error> {
    eprintln!("main");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("nix_otel_plugin")
        .enable_all()
        .build()
        .unwrap();
    eprintln!("runtime");
    let tracer = runtime.block_on(async {
        opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(opentelemetry_otlp::new_exporter().tonic().with_env())
            .with_trace_config(
                opentelemetry_sdk::trace::config()
                    .with_resource(Resource::new([KeyValue::new("service.name", "nix-otel")])),
            )
            .install_simple()
    })?;
    eprintln!("tracer init");
    Ok(Context {
        tracer,
        runtime,
        active_spans: Mutex::new(SpanMap::default()),
    })
}

fn startup() -> Context {
    make_context().unwrap()
}

#[no_mangle]
pub extern "C" fn initialize_plugin() -> *const Context {
    let context = startup();
    Box::into_raw(Box::new(context))
}
