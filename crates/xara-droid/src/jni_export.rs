#![allow(non_snake_case, clippy::too_many_arguments)]

use crate::app;
use crate::engine::StartReq;
use jni::JNIEnv;
use jni::objects::{JByteBuffer, JClass, JObject, JShortArray, JString};
use jni::sys::{jboolean, jint, jlong, jstring};

fn clear(env: &JNIEnv) {
    let _ = env.exception_clear();
}

fn java_str(env: &mut JNIEnv, value: &JString) -> Option<String> {
    match env.get_string(value) {
        Ok(s) => Some(s.into()),
        Err(_) => {
            clear(env);
            None
        }
    }
}

fn ret_string(env: &mut JNIEnv, s: &str) -> jstring {
    match env.new_string(s) {
        Ok(j) => j.into_raw(),
        Err(_) => {
            clear(env);
            std::ptr::null_mut()
        }
    }
}

fn direct(env: &JNIEnv, obj: JObject) -> Option<(*mut u8, usize)> {
    let buf = JByteBuffer::from(obj);
    let ptr = env.get_direct_buffer_address(&buf).ok()?;
    let cap = env.get_direct_buffer_capacity(&buf).ok()?;
    if ptr.is_null() && cap > 0 {
        return None;
    }
    Some((ptr, cap))
}

fn read_in(ptr: *mut u8, cap: usize, len: i32) -> Vec<u8> {
    let n = (len.max(0) as usize).min(cap);
    if n == 0 || ptr.is_null() {
        return Vec::new();
    }
    let mut v = vec![0u8; n];
    unsafe { std::ptr::copy_nonoverlapping(ptr, v.as_mut_ptr(), n) };
    v
}

fn write_out(ptr: *mut u8, cap: usize, src: &[u8]) -> usize {
    let n = src.len().min(cap);
    if n == 0 || ptr.is_null() {
        return 0;
    }
    unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), ptr, n) };
    n
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_ping(_env: JNIEnv, _class: JClass) -> jint {
    1
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_block(_env: JNIEnv, _class: JClass) -> jint {
    app::block_len() as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_start<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    call: JString<'local>,
    mode: jint,
    addr: JString<'local>,
    cmd_port: jint,
    kiss_port: jint,
    lead_ms: jint,
) -> jstring {
    let Some(call) = java_str(&mut env, &call) else { return ret_string(&mut env, "ERR\nmissing call") };
    let Some(addr) = java_str(&mut env, &addr) else { return ret_string(&mut env, "ERR\nmissing address") };
    let req = StartReq {
        call,
        mode,
        addr,
        cmd_port: cmd_port as u16,
        kiss_port: kiss_port as u16,
        lead_ms: lead_ms.max(0) as u32,
    };
    let msg = match app::start(&req) {
        Ok(line) => format!("OK\n{line}"),
        Err(e) => format!("ERR\n{e}"),
    };
    ret_string(&mut env, &msg)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_status<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    ret_string(&mut env, &app::status())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_process<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    rx: JShortArray<'local>,
    tx: JShortArray<'local>,
    force_off: jboolean,
) -> jint {
    let Ok(n) = env.get_array_length(&rx) else { return -2 };
    let Ok(m) = env.get_array_length(&tx) else { return -2 };
    if n <= 0 || m < n {
        return -2;
    }
    let mut input = vec![0i16; n as usize];
    if env.get_short_array_region(&rx, 0, &mut input).is_err() {
        clear(&env);
        return -2;
    }
    let mut output = vec![0i16; n as usize];
    let keyed = match app::process(&input, &mut output, force_off != 0) {
        Ok(k) => k,
        Err(_) => return -1,
    };
    if env.set_short_array_region(&tx, 0, &output).is_err() {
        clear(&env);
        return -2;
    }
    if keyed { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_setTail(_env: JNIEnv, _class: JClass, ms: jint) {
    app::set_tail(ms.max(0) as u32);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_setManual(_env: JNIEnv, _class: JClass, on: jboolean) {
    app::set_manual(on != 0);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_registered(_env: JNIEnv, _class: JClass) -> jboolean {
    u8::from(crate::registered())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_stop(_env: JNIEnv, _class: JClass) {
    app::stop();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_lastKeyed(_env: JNIEnv, _class: JClass) -> jboolean {
    u8::from(app::last_keyed())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_catOpen(
    _env: JNIEnv,
    _class: JClass,
    radio: jint,
    addr: jint,
) -> jint {
    match app::cat_open(radio, addr) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_radioBaud(_env: JNIEnv, _class: JClass, radio: jint) -> jint {
    app::radio_baud(radio)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_catBuild<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    op: jint,
    hz: jlong,
    mode: jint,
    dst: JObject<'local>,
) -> jint {
    let Some((ptr, cap)) = direct(&env, dst) else { return -2 };
    let hz = if hz < 0 { 0 } else { hz as u64 };
    let bytes = match app::cat_build(op, hz, mode) {
        Ok(b) => b,
        Err(_) => return -1,
    };
    write_out(ptr, cap, &bytes) as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_catFeed<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    src: JObject<'local>,
    len: jint,
    dst: JObject<'local>,
) -> jint {
    let Some((sp, scap)) = direct(&env, src) else { return -2 };
    let Some((dp, dcap)) = direct(&env, dst) else { return -2 };
    let inn = read_in(sp, scap, len);
    let mut out = vec![0u8; dcap];
    let n = app::cat_feed(&inn, &mut out);
    write_out(dp, dcap, &out[..n]) as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_trusdxEnable<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    speaker: jboolean,
    dst: JObject<'local>,
) -> jint {
    let Some((ptr, cap)) = direct(&env, dst) else { return -2 };
    write_out(ptr, cap, app::trusdx_enable(speaker != 0)) as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_endTx<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    dst: JObject<'local>,
) -> jint {
    let Some((ptr, cap)) = direct(&env, dst) else { return -2 };
    let mut out = vec![0u8; cap];
    let n = app::end_tx(&mut out);
    write_out(ptr, cap, &out[..n]) as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_x0rro_xara_NativeModem_cycleTrusdx<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    src: JObject<'local>,
    len: jint,
    force_off: jboolean,
    dst: JObject<'local>,
    cat: JObject<'local>,
) -> jlong {
    let Some((sp, scap)) = direct(&env, src) else { return -1 };
    let Some((dp, dcap)) = direct(&env, dst) else { return -1 };
    let Some((cp, ccap)) = direct(&env, cat) else { return -1 };
    let inn = read_in(sp, scap, len);
    let mut out = vec![0u8; dcap];
    let mut text = vec![0u8; ccap];
    match app::cycle_trusdx(&inn, force_off != 0, &mut out, &mut text) {
        Ok((n_out, n_cat, _)) => {
            let n_out = write_out(dp, dcap, &out[..n_out]);
            let n_cat = write_out(cp, ccap, &text[..n_cat]);
            ((n_cat as jlong) << 32) | n_out as jlong
        }
        Err(_) => -1,
    }
}
