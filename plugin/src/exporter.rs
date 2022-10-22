use std::{borrow::Cow, collections::HashMap, convert::TryFrom, time::SystemTime};

use opentelemetry::{
    global,
    trace::{TraceContextExt, Tracer as TracerT},
    Context, KeyValue,
};
use opentelemetry_api::trace::TracerProvider as TracerProviderT;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    trace::{Tracer, TracerProvider},
    Resource,
};
use tokio::{runtime::Runtime, sync::mpsc};
use tonic::metadata::{AsciiMetadataKey, AsciiMetadataValue, MetadataMap};

use crate::activity::{ActivityId, ActivityRecord, Field, ResultKind};

struct SpanMap(pub HashMap<ActivityId, ActivityData>);

struct ActivityData {
    // record: ActivityRecord,
    context: Context,
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
        let parent_context = record
            .parent
            .and_then(|p| self.0.get(&p))
            .map(|p| &p.context)
            .unwrap_or(context);

        let ad = ActivityData {
            // TODO: is this actually right?!
            context: parent_context.with_span(
                tracer
                    .span_builder(Cow::Owned(name))
                    .with_start_time(start_time)
                    .with_attributes([KeyValue::new(
                        "nix.activitykind",
                        format!("{:?}", record.kind),
                    )])
                    .start_with_context(tracer, parent_context),
            ),
        };

        self.0.insert(id, ad);
    }

    fn result(&mut self, act: ActivityId, kind: ResultKind, time: SystemTime) {
        if let Some(ad) = self.0.get(&act) {
            ad.context
                .span()
                .add_event_with_timestamp(format!("{kind:?}"), time, Vec::default())
        }
    }

    fn end(&mut self, act: ActivityId, end_time: SystemTime) {
        if let Some(ad) = self.0.get_mut(&act) {
            ad.context.span().end_with_timestamp(end_time);
            // intentionally don't remove it, since it may be used as a parent
            // for another span
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
    Result(ActivityId, ResultKind, SystemTime, Vec<Field>),
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
            span_map.begin(tracer, root_context, rec, time);
        }
        Message::EndActivity(id, time) => {
            span_map.end(id, time);
        }
        // FIXME(jade): use fields
        Message::Result(id, kind, time, _fields) => {
            span_map.result(id, kind, time);
        }
        Message::Terminate => panic!("handled outside"),
    }
}

fn get_tracer_headers(headers: String) -> MetadataMap {
    let mut map = MetadataMap::new();
    headers
        .split(',')
        .filter_map(|part| {
            let eq = part.find('=')?;
            let (key, val) = part.split_at(eq);
            let val = &val[1..];
            Some((
                AsciiMetadataKey::from_bytes(key.as_bytes()).ok()?,
                AsciiMetadataValue::try_from(val.as_bytes()).ok()?,
            ))
        })
        .for_each(|(h, val)| {
            let _ = map.insert(h, val);
        });
    map
}

async fn exporter_run(
    mut recv: mpsc::UnboundedReceiver<Message>,
    endpoint: Option<String>,
    otlp_headers: String,
) -> Result<(), Error> {
    let tracer = if let Some(endpoint) = endpoint {
        let tracer_meta = get_tracer_headers(otlp_headers);

        opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(
                opentelemetry_otlp::new_exporter()
                    .tonic()
                    .with_endpoint(endpoint)
                    .with_metadata(tracer_meta),
            )
            .with_trace_config(
                opentelemetry_sdk::trace::config()
                    .with_resource(Resource::new([KeyValue::new("service.name", "nix-otel")])),
            )
            .install_batch(opentelemetry_sdk::runtime::Tokio)?
    } else {
        TracerProvider::versioned_tracer(&TracerProvider::default(), "NullTracer", None, None)
    };

    // FIXME(jade): this sets a global tracing provider, which is probably
    // *wrong* for a plugin to do. We could definitely desugar this and not
    // do the global provider.

    let mut span_map = SpanMap::default();

    let root_context = Context::new();
    let root_context =
        root_context.with_span(tracer.start_with_context("nix execution", &root_context));
    loop {
        match recv.recv().await {
            None | Some(Message::Terminate) => {
                recv.close();
                // is this manual usage an async bug? who can say. the
                // background context stuff is a real good way to fuck up with
                // async though....
                root_context.span().end();
                break;
            }
            Some(v) => process_message(v, &mut span_map, &tracer, &root_context).await,
        }
    }
    if let Some(tp) = tracer.provider() {
        for res in tp.force_flush() {
            if let Err(e) = res {
                eprintln!("send error: {e:?}");
            }
        }
        drop(tp);
    }
    global::shutdown_tracer_provider();
    Ok(())
}

pub fn exporter_main(
    recv: mpsc::UnboundedReceiver<Message>,
    endpoint: Option<String>,
    otlp_headers: String,
) {
    let runtime = startup().expect("startup exporter");
    runtime
        .block_on(exporter_run(recv, endpoint, otlp_headers))
        .expect("fatal exporter error");
}
