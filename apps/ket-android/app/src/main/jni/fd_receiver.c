#include <jni.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

JNIEXPORT jint JNICALL
Java_com_ket_android_FdReceiver_receiveFileDescriptor(JNIEnv *env, jobject thiz,
                                                       jint socket_fd)
{
    char payload;
    struct iovec iov = {
        .iov_base = &payload,
        .iov_len = sizeof(payload),
    };
    union {
        struct cmsghdr align;
        char bytes[CMSG_SPACE(sizeof(int))];
    } control;
    struct msghdr message;

    (void)env;
    (void)thiz;
    memset(&message, 0, sizeof(message));
    memset(&control, 0, sizeof(control));
    message.msg_iov = &iov;
    message.msg_iovlen = 1;
    message.msg_control = control.bytes;
    message.msg_controllen = sizeof(control.bytes);

    if (recvmsg(socket_fd, &message, 0) < 0)
        return -1;
    if ((message.msg_flags & MSG_CTRUNC) != 0)
        return -1;

    for (struct cmsghdr *header = CMSG_FIRSTHDR(&message); header != NULL;
         header = CMSG_NXTHDR(&message, header)) {
        if (header->cmsg_level == SOL_SOCKET && header->cmsg_type == SCM_RIGHTS &&
            header->cmsg_len >= CMSG_LEN(sizeof(int))) {
            int descriptor;
            memcpy(&descriptor, CMSG_DATA(header), sizeof(descriptor));
            return descriptor;
        }
    }
    return -1;
}
