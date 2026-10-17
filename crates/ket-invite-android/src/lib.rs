//! JNI entry points for `com.ket.android.invite.NativeInvites`.
//!
//! Each function converts Java values, calls [`reader`], and converts the
//! result back. A JNI failure returns null, which the Kotlin side treats as
//! an invalid invite; no function panics across the boundary.

pub mod reader;

use jni::{
    JNIEnv,
    objects::{JByteArray, JClass, JObject, JString},
    sys::{jbyteArray, jlong, jobjectArray, jstring},
};

/// `readInvite(text, now): Array<String?>`.
///
/// Failure: `[code]`. Success: `[null, nodeKey, fingerprint, accessCode,
/// expiresAt or null, sealedControl ("1" or "0"), origin...]`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ket_android_invite_NativeInvites_readInvite<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    text: JString<'local>,
    now_epoch_seconds: jlong,
) -> jobjectArray {
    let Some(text) = java_string(&mut env, &text) else {
        return std::ptr::null_mut();
    };
    let row = match reader::read_invite(&text, now_epoch_seconds) {
        Ok(fields) => {
            let mut row = vec![
                None,
                Some(fields.node_key),
                Some(fields.node_fingerprint),
                Some(fields.access_code.to_string()),
                fields
                    .access_expires_at_epoch_seconds
                    .map(|expiry| expiry.to_string()),
                Some(if fields.sealed_control { "1" } else { "0" }.to_owned()),
            ];
            row.extend(fields.origins.into_iter().map(Some));
            row
        }
        Err(error) => vec![Some(error.code().to_owned())],
    };
    string_array(&mut env, &row).unwrap_or(std::ptr::null_mut())
}

/// `nodeKeyFingerprint(nodeKey): String?`, null for an invalid key.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ket_android_invite_NativeInvites_nodeKeyFingerprint<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    node_key: JString<'local>,
) -> jstring {
    java_string(&mut env, &node_key)
        .and_then(|key| reader::node_key_fingerprint(&key))
        .and_then(|fingerprint| env.new_string(fingerprint).ok())
        .map_or(std::ptr::null_mut(), JString::into_raw)
}

/// `verifyEnrollment(nodeKey, nonce, body, signature?): String?`, null when
/// the node signed this nonce and body, otherwise the rejection code.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ket_android_invite_NativeInvites_verifyEnrollment<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    node_key: JString<'local>,
    nonce: JString<'local>,
    body: JByteArray<'local>,
    signature: JString<'local>,
) -> jstring {
    let result = (|| {
        let node_key = java_string(&mut env, &node_key)?;
        let nonce = java_string(&mut env, &nonce)?;
        let body = env.convert_byte_array(&body).ok()?;
        let signature = if signature.is_null() {
            None
        } else {
            Some(java_string(&mut env, &signature)?)
        };
        Some(reader::verify_enrollment(
            &node_key,
            &nonce,
            &body,
            signature.as_deref(),
        ))
    })();
    let code = match result {
        Some(Ok(())) => return std::ptr::null_mut(),
        Some(Err(error)) => error.code(),
        None => ket_invite::InviteError::NodeIdentityMismatch.code(),
    };
    env.new_string(code)
        .map_or(std::ptr::null_mut(), JString::into_raw)
}

/// `sealRequest(nodeKey, plaintext): Array<ByteArray>?`: `[sealed, opener]`,
/// or null for an invalid key. The opener is key material.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ket_android_invite_NativeInvites_sealRequest<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    node_key: JString<'local>,
    plaintext: JByteArray<'local>,
) -> jobjectArray {
    (|| {
        let node_key = java_string(&mut env, &node_key)?;
        let plaintext = zeroize::Zeroizing::new(env.convert_byte_array(&plaintext).ok()?);
        let request = reader::seal_request(&node_key, &plaintext).ok()?;
        let sealed = env.byte_array_from_slice(&request.sealed).ok()?;
        let opener = env.byte_array_from_slice(&request.opener).ok()?;
        let array = env.new_object_array(2, "[B", JObject::null()).ok()?;
        env.set_object_array_element(&array, 0, &sealed).ok()?;
        env.set_object_array_element(&array, 1, &opener).ok()?;
        Some(array.into_raw())
    })()
    .unwrap_or(std::ptr::null_mut())
}

/// `openResponse(opener, sealed): ByteArray?`, null unless the pinned node
/// sealed this answer for the request `opener` belongs to.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ket_android_invite_NativeInvites_openResponse<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    opener: JByteArray<'local>,
    sealed: JByteArray<'local>,
) -> jbyteArray {
    (|| {
        let opener = zeroize::Zeroizing::new(env.convert_byte_array(&opener).ok()?);
        let sealed = env.convert_byte_array(&sealed).ok()?;
        let plaintext = reader::open_response(&opener, &sealed).ok()?;
        env.byte_array_from_slice(&plaintext)
            .ok()
            .map(JByteArray::into_raw)
    })()
    .unwrap_or(std::ptr::null_mut())
}

/// Whether the library is loaded and matches what the app expects.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ket_android_invite_NativeInvites_controlApiVersion<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jlong {
    ket_invite::CONTROL_API_VERSION.into()
}

fn java_string(env: &mut JNIEnv<'_>, value: &JString<'_>) -> Option<String> {
    if value.is_null() {
        return None;
    }
    env.get_string(value).ok().map(Into::into)
}

fn string_array(env: &mut JNIEnv<'_>, row: &[Option<String>]) -> Option<jobjectArray> {
    let length = i32::try_from(row.len()).ok()?;
    let array = env
        .new_object_array(length, "java/lang/String", JObject::null())
        .ok()?;
    for (index, value) in row.iter().enumerate() {
        if let Some(value) = value {
            let element = env.new_string(value).ok()?;
            env.set_object_array_element(&array, i32::try_from(index).ok()?, &element)
                .ok()?;
            env.delete_local_ref(element).ok()?;
        }
    }
    Some(array.into_raw())
}
