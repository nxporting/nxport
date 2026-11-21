// nn::os as the game calls it: threads, synchronization (mutexes, events, semaphores, condition variables, message
// queues), TLS, memory and time.

use std::sync::Mutex;
use crate::horizon::vi::*;
use crate::horizon::os::objects::*;
use crate::horizon::call_log::*;
use crate::horizon::hle::*;
pub(crate) mod objects;
pub(crate) mod memory;
pub(crate) mod threads;
pub(crate) mod suspend;
pub(crate) mod locks;
pub(crate) mod events;
pub(crate) mod time;
pub(crate) use events::*;
pub(crate) use locks::*;
pub(crate) use time::*;
pub(crate) use threads::*;
pub(crate) use memory::*;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex as StdMutex;





































































































































// ── misc nn::os ──────────────────────────────────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn set_dying_message_region(_addr: usize, _size: usize) {}




/// MultiWaitHolder user data: an opaque value the caller attaches to a holder.
pub(crate) static HOLDER_USER_DATA: Mutex<Option<HashMap<usize, usize>>> = Mutex::new(None);


/// Data-cache maintenance (FlushDataCache etc.): the sdk module's versions execute `dc`/`mrs ctr_el0`
/// sequences that Darwin traps (SIGILL). Apple Silicon memory is coherent for our purposes, so no-op.
pub(crate) unsafe extern "C" fn cache_noop(_addr: *const u8, _size: usize) {}
