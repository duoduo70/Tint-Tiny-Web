/* Tiny Tiny Web
 * Copyright (C) 2024 Plasma (https://github.com/duoduo70/Tiny-Tiny-Web/).
 *
 * You should have received a copy of the GNU General Public License
 * along with this program;
 * if not, see <https://www.gnu.org/licenses/>.
 */
use std::{process::exit, sync::{atomic::Ordering, Mutex, Arc}, collections::VecDeque, net::{TcpListener, TcpStream}};
use crate::{config::*, ReqCounter, drop::time::Time, TimeErr, i18n::LOG, handle_connection, ThreadPool};
use crate::marco::*;
use crate::drop::log::LogLevel::*;

struct StreamResultCounters {
    req_counter: ReqCounter,
    old_stamp: i16,
    new_stamp: i16,
    tmp_counter: u32,
    box_num_per_thread: u32,
    flag_new_box_num: bool,
    old_stamp_timeout: i16,
    new_stamp_timeout: i16,
}

static mut THREADS_BOX: Option<Arc<Mutex<VecDeque<std::net::TcpStream>>>> = None;

pub fn boxmode(listener: TcpListener, mut threadpool: ThreadPool) -> ! {
    
    match listener.set_nonblocking(true) {
        Err(_) => log!(Warn, LOG[26]),
        _ => (),
    }

    let box_num_per_thread_mag = BOX_NUM_PER_THREAD_MAG.load(Ordering::Relaxed) as f32 / 1000.0;
    let box_num_per_thread_init_mag =
        BOX_NUM_PER_THREAD_INIT_MAG.load(Ordering::Relaxed) as f32 / 1000.0;
    let xrps_predict_mag = XRPS_PREDICT_MAG.load(Ordering::Relaxed) as f32 / 1000.0;
    let threads_num = THREADS_NUM.load(Ordering::Relaxed);
    let mut counters = StreamResultCounters {
        req_counter: ReqCounter::new(),
        old_stamp: Time::msec().result_timeerr_default(),
        new_stamp: Time::msec().result_timeerr_default(),
        tmp_counter: 0,
        box_num_per_thread: threads_num * 3,
        flag_new_box_num: false,
        old_stamp_timeout: Time::msec().result_timeerr_default(),
        new_stamp_timeout: Time::msec().result_timeerr_default(),
    };
    unsafe { THREADS_BOX = Some(Arc::new(Mutex::new(VecDeque::new()))) };
    // TODO:The thread factory is not aligned based on the timeline, and the efficiency is not the highest
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                ok_vars_init(&mut counters);

                if let Err(()) = thread_box_add(&stream) {
                    continue;
                }

                log!(Debug, format!("{}{:#?}\n", LOG[3], stream));
                if is_nst_gt_ost_timeout(&counters.old_stamp_timeout, &counters.new_stamp_timeout) {
                    if_new_tick_start(&mut counters, xrps_predict_mag);
                    let func = move || {
                        let mut i = 0;
                        while i
                            != (counters.box_num_per_thread as f32 * box_num_per_thread_mag) as u32
                        {
                            handle_connection_s(
                                unsafe { &THREADS_BOX.clone().unwrap() },
                                &Arc::clone(unsafe { &GLOBAL_CONFIG.clone().unwrap() }),
                            );
                            i += 1;
                        }
                    };
                    threadpool.add(threads_num.try_into().unwrap(), func);
                    counters.old_stamp_timeout = counters.new_stamp_timeout;
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                err_vars_init(&mut counters);
                if_new_tick_start(&mut counters, xrps_predict_mag);
                if counters.flag_new_box_num
                    || is_nst_gt_ost_timeout(&counters.old_stamp, &counters.new_stamp)
                {
                    let func = move || {
                        let mut i = 0;
                        while i
                            != (counters.box_num_per_thread as f32 * box_num_per_thread_mag) as u32
                        {
                            handle_connection_s(
                                unsafe { &THREADS_BOX.clone().unwrap() },
                                &Arc::clone(unsafe { &GLOBAL_CONFIG.clone().unwrap() }),
                            );
                            i += 1;
                        }
                    };
                    threadpool.add(threads_num.try_into().unwrap(), func);
                    counters.box_num_per_thread =
                        (threads_num as f32 * box_num_per_thread_init_mag) as u32;
                }
                continue;
            }
            _ => log!(Error, LOG[2]),
        }
    }
    exit(0);
}

fn handle_connection_s(streams: &Mutex<VecDeque<std::net::TcpStream>>, config: &Mutex<Config>) {
    let stream = match streams.lock().unwrap().pop_front() {
        Some(a) => a,
        _ => return,
    };
    handle_connection(stream, &config);
}

fn err_vars_init(counters: &mut StreamResultCounters) {
    counters.new_stamp_timeout = Time::msec().result_timeerr_default();
    counters.new_stamp = counters.new_stamp_timeout;
}

fn ok_vars_init(counters: &mut StreamResultCounters) {
    counters.tmp_counter += 1;
    counters.new_stamp = Time::msec().result_timeerr_default();
    counters.flag_new_box_num = false;
}

fn if_new_tick_start(counters: &mut StreamResultCounters, xrps_predict_mag: f32) {
    if is_nst_gt_ost_helfsec(&counters.old_stamp, &counters.new_stamp) {
        counters.old_stamp = counters.new_stamp;
        counters.req_counter.change(counters.tmp_counter);
        counters.tmp_counter = 0;
        counters.box_num_per_thread =
            (counters.req_counter.get_xrps() as f32 * xrps_predict_mag) as u32;
        counters.flag_new_box_num = true;
    }
}

fn thread_box_add(stream: &TcpStream) -> Result<(), ()> {
    unsafe {
        THREADS_BOX
            .clone()
            .unwrap()
            .clone()
            .lock()
            .unwrap()
            .push_back(if let Ok(a) = stream.try_clone() {
                a
            } else {
                log!(Warn, LOG[27]);
                return Err(());
            });
    }
    Ok(())
}

fn is_nst_gt_ost_timeout(old_stamp: &i16, new_stamp: &i16) -> bool {
    let differ = new_stamp - old_stamp;
    if differ > 50 {
        true
    } else if differ < 0 && 1000 + differ > 50 {
        true
    } else {
        false
    }
}

fn is_nst_gt_ost_helfsec(old_stamp: &i16, new_stamp: &i16) -> bool {
    let differ = new_stamp - old_stamp;
    if differ > 500 {
        true
    } else if differ < 0 && 1000 + differ > 500 {
        true
    } else {
        false
    }
}