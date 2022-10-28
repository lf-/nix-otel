use std::{borrow::Cow, collections::HashMap, convert::TryFrom, time::SystemTime};

use opentelemetry::{
    global,
    trace::{TraceContextExt, Tracer as TracerT},
    Array, Context, KeyValue,
};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{trace::Tracer, Resource};
use tokio::{runtime::Runtime, sync::mpsc};
use tonic::metadata::{AsciiMetadataKey, AsciiMetadataValue, MetadataMap};

use crate::activity::{ActivityId, ActivityRecord, Field, ResultKind};

struct SpanMap {
    pub map: HashMap<ActivityId, ActivityData>,
    tracer: Tracer,
}

struct ActivityData {
    // record: ActivityRecord,
    context: Context,
    phase_span: Option<Context>,
}

fn fields_key_value(fields: &Vec<Field>) -> KeyValue {
    KeyValue::new(
        "nix.fields",
        opentelemetry::Value::Array(Array::String(
            fields.iter().map(|v| format!("{v}").into()).collect(),
        )),
    )
}

impl SpanMap {
    fn begin(&mut self, context: &Context, record: ActivityRecord, start_time: SystemTime) {
        let id = record.id;
        let name = record.name.clone();
        let parent_context = record
            .parent
            .and_then(|p| self.map.get(&p))
            .map(|p| &p.context)
            .unwrap_or(context);

        let attrs = [
            KeyValue::new("nix.activitykind", format!("{:?}", record.kind)),
            fields_key_value(&record.fields),
        ];

        let ad = ActivityData {
            // TODO: is this actually right?!
            context: parent_context.with_span(
                self.tracer
                    .span_builder(Cow::Owned(name))
                    .with_start_time(start_time)
                    .with_attributes(attrs)
                    .start_with_context(&self.tracer, parent_context),
            ),
            phase_span: None,
        };

        self.map.insert(id, ad);
    }

    fn result(&mut self, act: ActivityId, kind: ResultKind, time: SystemTime, fields: Vec<Field>) {
        if let Some(ad) = self.map.get_mut(&act) {
            let attrs = vec![
                KeyValue::new("nix.event_kind", format!("{kind:?}")),
                fields_key_value(&fields),
            ];
            match kind {
                ResultKind::SetPhase => {
                    if let Some(ref span) = ad.phase_span {
                        span.span().end_with_timestamp(time);
                    }
                    ad.phase_span = Some(
                        ad.context.with_span(
                            self.tracer
                                .span_builder(Cow::Owned(
                                    fields
                                        .get(0)
                                        .map(|f| format!("{f}"))
                                        .unwrap_or("no phase".to_owned()),
                                ))
                                .start_with_context(&self.tracer, &ad.context),
                        ),
                    )
                }
                ResultKind::BuildLogLine => ad.context.span().add_event_with_timestamp(
                    format!(
                        "{}",
                        fields
                            .get(0)
                            .unwrap_or(&Field::String("(no message)".to_string())),
                    ),
                    time,
                    attrs,
                ),
                kind => {
                    ad.context
                        .span()
                        .add_event_with_timestamp(format!("{kind:?}"), time, attrs)
                }
            }
        }
    }

    fn end(&mut self, act: ActivityId, end_time: SystemTime) {
        if let Some(ad) = self.map.get_mut(&act) {
            if let Some(ref span) = ad.phase_span {
                span.span().end_with_timestamp(end_time);
            }
            ad.context.span().end_with_timestamp(end_time);
            // intentionally don't remove it, since it may be used as a parent
            // for another span
        }
    }
}
impl SpanMap {
    fn new(tracer: Tracer) -> Self {
        Self {
            map: Default::default(),
            tracer,
        }
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

async fn process_message(message: Message, span_map: &mut SpanMap, root_context: &Context) {
    match message {
        Message::BeginActivity(rec, time) => {
            span_map.begin(root_context, rec, time);
        }
        Message::EndActivity(id, time) => {
            span_map.end(id, time);
        }
        Message::Result(id, kind, time, fields) => {
            span_map.result(id, kind, time, fields);
        }
        Message::Terminate => panic!("handled outside"),
    }
}

fn get_tracer_headers() -> MetadataMap {
    let mut map = MetadataMap::new();
    let headers = std::env::var("OTEL_EXPORTER_OTLP_HEADERS");
    if let Ok(h) = headers {
        h.split(',')
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
    }
    map
}

async fn exporter_run(mut recv: mpsc::UnboundedReceiver<Message>) -> Result<(), Error> {
    let tracer_meta = get_tracer_headers();
    // FIXME(jade): this sets a global tracing provider, which is probably
    // *wrong* for a plugin to do. We could definitely desugar this and not
    // do the global provider.
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_env()
                .with_metadata(tracer_meta),
        )
        .with_trace_config(
            opentelemetry_sdk::trace::config()
                .with_resource(Resource::new([KeyValue::new("service.name", "nix-otel")])),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)?;

    let mut span_map = SpanMap::new(tracer.clone());

    let root_context = Context::new();
    let root_context =
        root_context.with_span(tracer.start_with_context(
                std::env::args().collect::<Vec<String>>().join(" "),
                &root_context));
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
            Some(v) => process_message(v, &mut span_map, &root_context).await,
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

pub fn exporter_main(recv: mpsc::UnboundedReceiver<Message>) {
    let runtime = startup().expect("startup exporter");
    runtime
        .block_on(exporter_run(recv))
        .expect("fatal exporter error");
}
