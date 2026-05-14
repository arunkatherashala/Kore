//! Java/JNI bindings for cloud connectors
//! 
//! Exposes S3Reader, AzureBlobReader, and GcsReader to Java via JNI
//! 
//! Build: cargo build --release --features java
//! Expected output: target/release/kore_java.dll (Windows) or .so (Linux) or .dylib (macOS)

#![cfg(feature = "java")]

use jni::JNIEnv;
use jni::objects::{JClass, JString, JObject};
use jni::sys::{jstring, jobject, jint, jbyteArray};

#[cfg(feature = "s3")]
use crate::s3_reader::S3Reader;

#[cfg(feature = "azure")]
use crate::azure_reader::AzureBlobReader;

#[cfg(feature = "gcs")]
use crate::gcs_reader::GcsReader;

// ============================================================================
// S3Reader JNI Bindings
// ============================================================================

#[cfg(feature = "s3")]
#[no_mangle]
pub extern "C" fn Java_com_kore_cloud_S3Reader_newInstance(
    mut env: JNIEnv,
    _class: JClass,
    region: JString,
) -> jobject {
    let region_str: String = match env.get_string(&region) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };

    match S3Reader::new(&region_str) {
        Ok(reader) => {
            let boxed = Box::new(reader);
            let ptr = Box::into_raw(boxed) as *mut _ as i64;
            match env.new_long_array(1) {
                Ok(arr) => {
                    let _ = env.set_long_array_region(&arr, 0, &[ptr]);
                    arr.into_raw()
                }
                Err(_) => std::ptr::null_mut(),
            }
        }
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(feature = "s3")]
#[no_mangle]
pub extern "C" fn Java_com_kore_cloud_S3Reader_readFile(
    _env: JNIEnv,
    _obj: JObject,
    bucket: JString,
    key: JString,
) -> jbyteArray {
    // Implementation requires async runtime bridge
    // For now, return placeholder
    // Production: Use tokio::runtime to bridge async to sync
    std::ptr::null_mut()
}

#[cfg(feature = "s3")]
#[no_mangle]
pub extern "C" fn Java_com_kore_cloud_S3Reader_writeFile(
    _env: JNIEnv,
    _obj: JObject,
    bucket: JString,
    key: JString,
    data: jbyteArray,
) -> jint {
    0 // Success placeholder
}

#[cfg(feature = "s3")]
#[no_mangle]
pub extern "C" fn Java_com_kore_cloud_S3Reader_listFiles(
    _env: JNIEnv,
    _obj: JObject,
    bucket: JString,
    prefix: JString,
) -> jobject {
    // Return Java String[] or List<String>
    std::ptr::null_mut()
}

#[cfg(feature = "s3")]
#[no_mangle]
pub extern "C" fn Java_com_kore_cloud_S3Reader_cleanup(
    _env: JNIEnv,
    _obj: JObject,
    ptr: i64,
) {
    if ptr != 0 {
        let _ = unsafe { Box::from_raw(ptr as *mut S3Reader) };
    }
}

// ============================================================================
// AzureBlobReader JNI Bindings
// ============================================================================

#[cfg(feature = "azure")]
#[no_mangle]
pub extern "C" fn Java_com_kore_cloud_AzureBlobReader_newInstance(
    mut env: JNIEnv,
    _class: JClass,
    account: JString,
    key: JString,
) -> jobject {
    let account_str: String = match env.get_string(&account) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let key_str: String = match env.get_string(&key) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };

    match AzureBlobReader::new(&account_str, &key_str) {
        Ok(reader) => {
            let boxed = Box::new(reader);
            let ptr = Box::into_raw(boxed) as *mut _ as i64;
            match env.new_long_array(1) {
                Ok(arr) => {
                    let _ = env.set_long_array_region(&arr, 0, &[ptr]);
                    arr.into_raw()
                }
                Err(_) => std::ptr::null_mut(),
            }
        }
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(feature = "azure")]
#[no_mangle]
pub extern "C" fn Java_com_kore_cloud_AzureBlobReader_readFile(
    _env: JNIEnv,
    _obj: JObject,
    container: JString,
    blob_path: JString,
) -> jbyteArray {
    // Bridge async to sync via tokio runtime
    std::ptr::null_mut()
}

#[cfg(feature = "azure")]
#[no_mangle]
pub extern "C" fn Java_com_kore_cloud_AzureBlobReader_cleanup(
    _env: JNIEnv,
    _obj: JObject,
    ptr: i64,
) {
    if ptr != 0 {
        let _ = unsafe { Box::from_raw(ptr as *mut AzureBlobReader) };
    }
}

// ============================================================================
// GcsReader JNI Bindings
// ============================================================================

#[cfg(feature = "gcs")]
#[no_mangle]
pub extern "C" fn Java_com_kore_cloud_GcsReader_newInstance(
    mut env: JNIEnv,
    _class: JClass,
    project_id: JString,
) -> jobject {
    let project_str: String = match env.get_string(&project_id) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };

    match GcsReader::new(&project_str) {
        Ok(reader) => {
            let boxed = Box::new(reader);
            let ptr = Box::into_raw(boxed) as *mut _ as i64;
            match env.new_long_array(1) {
                Ok(arr) => {
                    let _ = env.set_long_array_region(&arr, 0, &[ptr]);
                    arr.into_raw()
                }
                Err(_) => std::ptr::null_mut(),
            }
        }
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(feature = "gcs")]
#[no_mangle]
pub extern "C" fn Java_com_kore_cloud_GcsReader_readFile(
    _env: JNIEnv,
    _obj: JObject,
    bucket: JString,
    object_path: JString,
) -> jbyteArray {
    // Bridge async to sync via tokio runtime
    std::ptr::null_mut()
}

#[cfg(feature = "gcs")]
#[no_mangle]
pub extern "C" fn Java_com_kore_cloud_GcsReader_cleanup(
    _env: JNIEnv,
    _obj: JObject,
    ptr: i64,
) {
    if ptr != 0 {
        let _ = unsafe { Box::from_raw(ptr as *mut GcsReader) };
    }
}
