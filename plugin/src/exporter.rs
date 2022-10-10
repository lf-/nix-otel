use std::{borrow::Cow, collections::HashMap, time::SystemTime};

use opentelemetry::{
    trace::{Span as SpanT, Tracer as TracerT},
    Context, KeyValue,
};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    trace::{Span, Tracer},
    Resource,
};
use tokio::{runtime::Runtime, sync::mpsc};

use crate::activity::{ActivityId, ActivityRecord};

struct SpanMap(pub HashMap<ActivityId, ActivityData>);

struct ActivityData {
    // record: ActivityRecord,
    span: Span,
}

impl SpanMap {
    fn begin(
        &mut self,
        tracer: &Tracer,
        context: &Context,
        record: ActivityRecord,
        start_time: SystemTime,
    ) {
        let id = record.id;
        let name = record.name.clone();
        let ad = ActivityData {
            span: tracer
                .span_builder(Cow::Owned(name))
                .with_start_time(start_time)
                .start_with_context(tracer, context),
        };

        self.0.insert(id, ad);
    }

    fn end(&mut self, act: ActivityId, end_time: SystemTime) {
        if let Some(ad) = self.0.get_mut(&act) {
            ad.span.end_with_timestamp(end_time);
            self.0.remove(&act);
        }
    }
}
impl Default for SpanMap {
    fn default() -> Self {
        Self(Default::default())
    }
}

#[derive(Debug)]
pub enum Message {
    BeginActivity(ActivityRecord, SystemTime),
    EndActivity(ActivityId, SystemTime),
    Terminate,
}

type Error = Box<dyn std::error::Error>;

fn startup() -> Result<Runtime, Error> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("nix_otel_plugin")
        .enable_all()
        .build()
        .unwrap();
    Ok(runtime)
}

async fn process_message(
    message: Message,
    span_map: &mut SpanMap,
    tracer: &Tracer,
    root_context: &Context,
) {
    match message {
        Message::BeginActivity(rec, time) => {
            eprintln!("start {rec:?} {time:?}");
            span_map.begin(tracer, root_context, rec, time);
        }
        Message::EndActivity(id, time) => {
            eprintln!("end {id:?} {time:?}");
            span_map.end(id, time);
        }
        Message::Terminate => panic!("handled outside"),
    }
}

async fn exporter_run(mut recv: mpsc::UnboundedReceiver<Message>) -> Result<(), Error> {
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(opentelemetry_otlp::new_exporter().tonic().with_env())
        .with_trace_config(
            opentelemetry_sdk::trace::config()
                .with_resource(Resource::new([KeyValue::new("service.name", "nix-otel")])),
        )
        .install_simple()?;

    let mut span_map = SpanMap::default();

    let root_context = Context::new();
    let mut root_span = tracer.start_with_context("nix execution", &root_context);
    loop {
        match recv.recv().await {
            None | Some(Message::Terminate) => {
                recv.close();
                // is this manual usage an async bug? who can say. the
                // background context stuff is a real good way to fuck up with
                // async though....
                root_span.end();
                return Ok(());
            }
            Some(v) => process_message(v, &mut span_map, &tracer, &root_context).await,
        }
    }
}

pub fn exporter_main(recv: mpsc::UnboundedReceiver<Message>) {
    let runtime = startup().expect("startup exporter");
    runtime
        .block_on(exporter_run(recv))
        .expect("fatal exporter error");
}
