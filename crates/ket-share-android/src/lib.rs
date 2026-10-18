//! JNI entry points for `com.ket.android.share.NativeShare`.
//!
//! Each function converts Java values, calls [`registry`], and converts the
//! result back. A JNI failure returns null or does nothing; no function
//! panics across the boundary.

pub mod egress;
pub mod registry;

use std::net::Ipv4Addr;

use jni::{
    JNIEnv,
    objects::{JClass, JObject, JString},
    sys::{jboolean, jlong, jlongArray, jobjectArray},
};

/// `start(address): Array<String>?`. Success: `["ok", handle, port,
/// username, password]`; failure: `["error", code]`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ket_android_share_NativeShare_start<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    address: JString<'local>,
) -> jobjectArray {
    let Some(address) = java_string(&mut env, &address) else {
        return std::ptr::null_mut();
    };
    let row = match address
        .parse::<Ipv4Addr>()
        .map_err(|_| registry::StartError::NotPrivate)
        .and_then(registry::start)
    {
        Ok(started) => vec![
            "ok".to_owned(),
            started.handle.to_string(),
            started.port.to_string(),
            started.credentials.username().to_owned(),
            started.credentials.password().to_owned(),
        ],
        Err(error) => vec!["error".to_owned(), error.code().to_owned()],
    };
    string_array(&mut env, &row).unwrap_or(std::ptr::null_mut())
}

/// `setNetwork(handle, network): Boolean`: the VPN's network handle, or 0
/// while it is down.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ket_android_share_NativeShare_setNetwork<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    network: jlong,
) -> jboolean {
    registry::set_network(handle, network as u64).into()
}

/// `stats(handle): LongArray?`: `[devices, connections, refusedSignIns]`, or
/// null once the share is stopped.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ket_android_share_NativeShare_stats<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jlongArray {
    (|| {
        let stats = registry::stats(handle)?;
        let values = [
            i64::try_from(stats.devices).ok()?,
            i64::try_from(stats.connections).ok()?,
            i64::try_from(stats.refused_sign_ins).ok()?,
        ];
        let array = env.new_long_array(3).ok()?;
        env.set_long_array_region(&array, 0, &values).ok()?;
        Some(array.into_raw())
    })()
    .unwrap_or(std::ptr::null_mut())
}

/// `stop(handle)`: closes the port and every connection.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ket_android_share_NativeShare_stop<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    registry::stop(handle);
}

fn java_string(env: &mut JNIEnv<'_>, value: &JString<'_>) -> Option<String> {
    if value.is_null() {
        return None;
    }
    env.get_string(value).ok().map(Into::into)
}

fn string_array(env: &mut JNIEnv<'_>, row: &[String]) -> Option<jobjectArray> {
    let length = i32::try_from(row.len()).ok()?;
    let array = env
        .new_object_array(length, "java/lang/String", JObject::null())
        .ok()?;
    for (index, value) in row.iter().enumerate() {
        let element = env.new_string(value).ok()?;
        env.set_object_array_element(&array, i32::try_from(index).ok()?, &element)
            .ok()?;
        env.delete_local_ref(element).ok()?;
    }
    Some(array.into_raw())
}
