use std::{
    ffi::CStr,
    os::raw::c_char,
    sync::Mutex,
    thread::{self, JoinHandle},
    time::SystemTime,
};

use activity::{ActivityId, ActivityKind};
use exporter::{exporter_main, Message};
use thread_local::ThreadLocal;
use tokio::sync::mpsc;

use crate::activity::ActivityRecord;
mod activity;
mod exporter;

pub struct Context {
    /// This is `Some` always, except when the system is shut down
    exporter_thread: Option<JoinHandle<()>>,
    /// Channels to send stuff to the exporter thread
    channels: ThreadLocal<mpsc::UnboundedSender<Message>>,
    /// Parent channel to clone into the thread local channels
    parent_channel: Mutex<mpsc::UnboundedSender<Message>>,
}

fn exporter_channel(cx: &Context) -> &mpsc::UnboundedSender<Message> {
    cx.channels.get_or(|| {
        let chan = cx.parent_channel.lock().unwrap();
        chan.clone()
    })
}

fn tell(cx: &Context, message: Message) {
    let chan = exporter_channel(cx);
    chan.send(message).expect("sending record");
}

#[no_mangle]
pub extern "C" fn start_activity(
    // FIXME: this lifetime is *basically* true but its also kinda evil
    cx: &Context,
    act: ActivityId,
    ty: ActivityKind,
    name: *const c_char,
    parent: ActivityId,
) {
    let name = unsafe { CStr::from_ptr(name as *const _) };
    let name_ = name.to_str().unwrap().to_owned();
    tell(
        cx,
        Message::BeginActivity(
            ActivityRecord {
                id: act,
                kind: ty,
                name: name_,
            },
            SystemTime::now(),
        ),
    );
}

#[no_mangle]
pub extern "C" fn end_activity(cx: &Context, act: ActivityId) {
    tell(cx, Message::EndActivity(act, SystemTime::now()))
}

#[no_mangle]
pub extern "C" fn initialize_plugin() -> *const Context {
    let (send, recv) = mpsc::unbounded_channel();
    let exporter_thread = thread::Builder::new()
        .name("OTel exporter thread".to_owned())
        .spawn(|| exporter_main(recv))
        .expect("startup exporter thread");

    Box::into_raw(Box::new(Context {
        exporter_thread: Some(exporter_thread),
        channels: ThreadLocal::new(),
        parent_channel: Mutex::new(send),
    }))
}

/// SAFETY:
/// The invariant that "cx" is exclusively available here is maintained by the
/// other side of the FFI. Beware.
#[no_mangle]
pub extern "C" fn deinitialize_plugin(cx: &mut Context) {
    cx.parent_channel
        .lock()
        .unwrap()
        .send(Message::Terminate)
        .unwrap();

    // can't actually force the thread to terminate if something bad happens,
    // but we can join it
    if let Some(join_handle) = cx.exporter_thread.take() {
        join_handle.join().expect("panic in exporter thread");
    }
}
