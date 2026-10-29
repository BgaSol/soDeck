use rquickjs::{Context, Function, Runtime, TypedArray};
use sodeck_communication::binary::{Frame as BinaryFrame, Queue as BinaryQueue};
use serde_json::Value;
use std::{
    collections::{HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub(crate) struct Engine {
    context: Context,
    runtime: Runtime,
    deadline: Arc<Mutex<Instant>>,
    outgoing: Arc<Mutex<VecDeque<Value>>>,
    binary: Arc<Mutex<BinaryQueue>>,
    rejections: Arc<Mutex<HashSet<usize>>>,
    rejection_overflow: Arc<AtomicBool>,
    loaded: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn on_js_stack(test: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(4 * 1024 * 1024)
            .spawn(test)
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn public_clock_survives_wall_clock_changes_and_is_frozen() {
        on_js_stack(|| {
            let mut engine = Engine::new(Arc::new(AtomicBool::new(false))).unwrap();
            engine.receive(json!({"kind":"initialize","id":"init","context":{},"code":"globalThis.sodeckPlugin={invoke(){const a=sodeck.clock.monotonicMs();Date.now=()=>-999999;const b=sodeck.clock.monotonicMs();return {a,b,frozen:Object.isFrozen(sodeck.clock)}}}"})).unwrap();
            engine.tick().unwrap(); engine.drain().unwrap();
            engine.receive(json!({"kind":"invoke","id":"clock","command":"clock"})).unwrap(); engine.tick().unwrap();
            let frames=engine.drain().unwrap();let value=&frames[0]["result"];
            assert!(value["b"].as_f64().unwrap()>=value["a"].as_f64().unwrap());
            assert_eq!(value["frozen"],true);
        });
    }
    #[test]
    fn lifecycle_and_asynchronous_core_roundtrip() {
        on_js_stack(|| {
            let mut engine = Engine::new(Arc::new(AtomicBool::new(false))).unwrap();
            engine.receive(json!({"kind":"initialize","id":"init","context":{},"code":"globalThis.sodeckPlugin = { prepare(){globalThis.prepared=true}, async invoke(command,params){ if(!prepared) throw Error('not prepared'); return await sodeck.call(command,params); }, stop(){} };"})).unwrap();
            engine.tick().unwrap();
            assert_eq!(engine.drain().unwrap()[0]["result"]["ready"], true);
            engine.receive(json!({"kind":"invoke","id":"command","command":"settings.get","params":{"key":"fontSize"}})).unwrap();
            engine.tick().unwrap();
            let outgoing = engine.drain().unwrap();
            assert_eq!(outgoing.len(), 1);
            assert_eq!(outgoing[0]["kind"], "core");
            engine
                .receive(json!({"kind":"result","id":outgoing[0]["id"],"result":{"value":14}}))
                .unwrap();
            engine.tick().unwrap();
            let response = engine.drain().unwrap();
            assert_eq!(response[0]["id"], "command");
            assert_eq!(response[0]["result"]["value"], 14);
            engine.receive(json!({"kind":"stop","id":"stop"})).unwrap();
            engine.tick().unwrap();
            assert_eq!(engine.drain().unwrap()[0]["result"]["stopped"], true);
        });
    }

    #[test]
    fn worker_exception_is_structured_and_host_survives() {
        on_js_stack(|| {
            let mut engine = Engine::new(Arc::new(AtomicBool::new(false))).unwrap();
            engine.receive(json!({"kind":"initialize","id":"init","context":{},"code":"globalThis.sodeckPlugin={invoke(){throw {code:'INVALID_ARGUMENT',message:'bad size'}}}"})).unwrap();
            engine.tick().unwrap();
            engine.drain().unwrap();
            engine
                .receive(json!({"kind":"invoke","id":"a","command":"invalid"}))
                .unwrap();
            engine.tick().unwrap();
            assert_eq!(
                engine.drain().unwrap()[0]["error"]["code"],
                "INVALID_ARGUMENT"
            );
            engine.tick().unwrap();
        });
    }

    #[test]
    fn infinite_promise_jobs_share_one_execution_deadline() {
        on_js_stack(|| {
            let mut engine = Engine::new(Arc::new(AtomicBool::new(false))).unwrap();
            engine.receive(json!({"kind":"initialize","id":"init","context":{},"code":"globalThis.sodeckPlugin={async invoke(){while(true) await Promise.resolve()}}"})).unwrap();
            engine.tick().unwrap();
            engine.drain().unwrap();
            engine
                .receive(json!({"kind":"invoke","id":"loop","command":"loop"}))
                .unwrap();
            let start = Instant::now();
            assert!(engine.tick().is_err());
            assert!(start.elapsed() < Duration::from_secs(8));
        });
    }
}

impl Engine {
    pub fn new(disconnected: Arc<AtomicBool>) -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        runtime.set_memory_limit(128 * 1024 * 1024);
        runtime.set_max_stack_size(1024 * 1024);
        let deadline = Arc::new(Mutex::new(Instant::now() + Duration::from_secs(5)));
        let clock = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            disconnected.load(Ordering::SeqCst) || Instant::now() > *clock.lock().unwrap()
        })));
        let rejections = Arc::new(Mutex::new(HashSet::new()));
        let rejection_overflow = Arc::new(AtomicBool::new(false));
        let rejected = rejections.clone();
        let overflow = rejection_overflow.clone();
        runtime.set_host_promise_rejection_tracker(Some(Box::new(
            move |_, promise, _, handled| {
                let id = unsafe { rquickjs::qjs::JS_VALUE_GET_PTR(promise.as_raw()) as usize };
                let mut set = rejected.lock().unwrap();
                if handled {
                    set.remove(&id);
                } else if set.len() < 1024 {
                    set.insert(id);
                } else {
                    overflow.store(true, Ordering::SeqCst);
                }
            },
        )));
        let context = Context::full(&runtime).map_err(|e| e.to_string())?;
        let outgoing = Arc::new(Mutex::new(VecDeque::new()));
        let queue = outgoing.clone();
        let binary = Arc::new(Mutex::new(BinaryQueue::default()));
        let binary_send = binary.clone();
        let start = Instant::now();
        context
            .with(|ctx| -> rquickjs::Result<()> {
                ctx.globals().set("__nativeBinarySend", Function::new(ctx.clone(), move |bytes: TypedArray<u8>| -> bool {
                    // No JS call or mutation occurs while this slice is borrowed;
                    // decode copies it before returning to the QuickJS event loop.
                    let Some(bytes) = (unsafe { bytes.as_bytes() }) else { return false };
                    let Ok(frame) = BinaryFrame::decode(bytes) else { return false };
                    binary_send.lock().unwrap().push(frame).unwrap_or(false)
                })?)?;
                ctx.globals().set(
                    "__nativeSend",
                    Function::new(ctx.clone(), move |text: String| -> bool {
                        if text.len() > super::transport::MAX_FRAME {
                            return false;
                        }
                        let Ok(frame) = serde_json::from_str::<Value>(&text) else {
                            return false;
                        };
                        let mut queue = queue.lock().unwrap();
                        if queue.len() >= 64 {
                            return false;
                        }
                        queue.push_back(frame);
                        true
                    })?,
                )?;
                ctx.globals().set(
                    "__nativeNow",
                    Function::new(ctx.clone(), move || start.elapsed().as_millis() as f64)?,
                )?;
                ctx.eval::<(), _>(include_str!("bootstrap.js"))
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            context,
            runtime,
            deadline,
            outgoing,
            binary,
            rejections,
            rejection_overflow,
            loaded: false,
        })
    }

    fn budget(&self) {
        *self.deadline.lock().unwrap() = Instant::now() + Duration::from_secs(5);
    }
    fn check_budget(&self) -> Result<(), String> {
        if Instant::now() > *self.deadline.lock().unwrap() {
            Err("Plugin execution exceeded 5 seconds".into())
        } else {
            Ok(())
        }
    }
    pub fn receive(&mut self, mut frame: Value) -> Result<(), String> {
        self.budget();
        if frame["kind"] == "initialize" {
            if self.loaded {
                return Err("Repeated plugin initialization".into());
            }
            let code = frame["code"]
                .as_str()
                .ok_or("Missing worker code")?
                .to_owned();
            self.context
                .with(|ctx| ctx.eval::<(), _>(code))
                .map_err(|e| format!("Worker evaluation failed: {e}"))?;
            self.loaded = true;
            frame
                .as_object_mut()
                .ok_or("Invalid initialization")?
                .remove("code");
        } else if !self.loaded {
            return Err("Plugin is not initialized".into());
        }
        let text = serde_json::to_string(&frame).map_err(|e| e.to_string())?;
        self.context
            .with(|ctx| -> rquickjs::Result<()> {
                let receive: Function = ctx.globals().get("__receive")?;
                receive.call((text,))
            })
            .map_err(|e| e.to_string())?;
        self.check_budget()
    }
    pub fn tick(&self) -> Result<(), String> {
        self.budget();
        self.context
            .with(|ctx| -> rquickjs::Result<()> {
                ctx.globals().get::<_, Function>("__tick")?.call(())
            })
            .map_err(|e| e.to_string())?;
        loop {
            let more = self
                .runtime
                .execute_pending_job()
                .map_err(|_| "Plugin asynchronous job failed")?;
            self.check_budget()?;
            if !more {
                break;
            }
        }
        if self.rejection_overflow.load(Ordering::SeqCst)
            || !self.rejections.lock().unwrap().is_empty()
        {
            return Err("Unhandled plugin rejection".into());
        }
        Ok(())
    }
    pub fn drain(&self) -> Result<Vec<Value>, String> {
        Ok(self.outgoing.lock().unwrap().drain(..).collect())
    }
    pub fn receive_binary(&mut self, frame: BinaryFrame) -> Result<(), String> {
        if !self.loaded { return Err("Binary stream before initialization".into()); }
        self.budget();
        let bytes = frame.encode().map_err(|e|e.to_string())?;
        self.context.with(|ctx| -> rquickjs::Result<()> {
            let receive:Function=ctx.globals().get("__receiveBinary")?;
            receive.call((TypedArray::new_copy(ctx,bytes)?,))
        }).map_err(|e|e.to_string())?;
        self.check_budget()
    }
    pub fn drain_binary(&self) -> Vec<BinaryFrame> {
        let mut queue=self.binary.lock().unwrap(); let mut bytes=0; let mut frames=Vec::new();
        while bytes < 256*1024 {let Some(frame)=queue.pop() else {break};bytes+=frame.size();frames.push(frame);}
        frames
    }
}
