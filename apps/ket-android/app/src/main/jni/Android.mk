LOCAL_PATH := $(call my-dir)
KET_LOCAL_PATH := $(LOCAL_PATH)

include $(LOCAL_PATH)/../../../build/generated/ket-engines/hev-socks5-tunnel/Android.mk

LOCAL_PATH := $(KET_LOCAL_PATH)
include $(CLEAR_VARS)
LOCAL_MODULE := ket-android-native
LOCAL_SRC_FILES := fd_receiver.c
LOCAL_LDFLAGS += -Wl,-z,max-page-size=16384
LOCAL_LDFLAGS += -Wl,-z,common-page-size=16384
include $(BUILD_SHARED_LIBRARY)
