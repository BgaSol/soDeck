#include <libssh2.h>

typedef LIBSSH2_SEND_FUNC((*send_fn));
typedef LIBSSH2_RECV_FUNC((*recv_fn));

/* Keep the platform ABI and legacy/system libssh2 compatibility in C. */
void sodeck_install_io(LIBSSH2_SESSION *session, send_fn send, recv_fn recv,
                      send_fn *old_send, recv_fn *old_recv) {
#if LIBSSH2_VERSION_NUM >= 0x010B01
    *old_send = (send_fn)libssh2_session_callback_set2(
        session, LIBSSH2_CALLBACK_SEND, (libssh2_cb_generic *)send);
    *old_recv = (recv_fn)libssh2_session_callback_set2(
        session, LIBSSH2_CALLBACK_RECV, (libssh2_cb_generic *)recv);
#else
    *old_send = (send_fn)libssh2_session_callback_set(
        session, LIBSSH2_CALLBACK_SEND, (void *)send);
    *old_recv = (recv_fn)libssh2_session_callback_set(
        session, LIBSSH2_CALLBACK_RECV, (void *)recv);
#endif
}
