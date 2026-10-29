/* SoDeck T-019 local adaptation. libssh2 license applies; see SODECK-PATCH.md. */
#include "libssh2_priv.h"
#include "transport.h"
#include "packet.h"

LIBSSH2_API void sodeck_session_observer(LIBSSH2_SESSION *s, void *ctx,
                                        sodeck_observer callback)
{
    s->sodeck_context = ctx;
    s->sodeck_observe = callback;
}

LIBSSH2_API void sodeck_channel_origin(LIBSSH2_SESSION *s,
                                      libssh2_uint64_t token)
{
    s->sodeck_origin = token;
}

/* Only the final Rust owner uses this: no channel handle can still be used.
 * Never wait for a peer while destroying the callback's owning allocation. */
LIBSSH2_API int sodeck_session_free_local(LIBSSH2_SESSION *s)
{
    s->sodeck_disposing = 1;
    s->socket_state = LIBSSH2_SOCKET_DISCONNECTED;
    libssh2_session_set_blocking(s, 0);
    return libssh2_session_free(s);
}

LIBSSH2_API libssh2_uint64_t sodeck_channel_token(LIBSSH2_CHANNEL *c)
{
    return c->sodeck_token;
}

void _sodeck_channel_observe(LIBSSH2_CHANNEL *c, int event, size_t count)
{
    LIBSSH2_SESSION *s = c->session;
    if(s->sodeck_observe)
        s->sodeck_observe(s->sodeck_context, c->sodeck_token,
                          c->local.id, event, count);
}

ssize_t _sodeck_send(LIBSSH2_SESSION *s, const void *buf, size_t len, int flags)
{
    if(s->sodeck_disposing)
        return -EIO;
    ssize_t rc = LIBSSH2_SEND_FD(s, s->socket_fd, buf, len, flags);
    if(rc > 0 && s->sodeck_observe)
        s->sodeck_observe(s->sodeck_context, 0, 0, 5, (size_t)rc);
    return rc;
}

ssize_t _sodeck_recv(LIBSSH2_SESSION *s, void *buf, size_t len, int flags)
{
    if(s->sodeck_disposing)
        return 0;
    ssize_t rc = LIBSSH2_RECV_FD(s, s->socket_fd, buf, len, flags);
    if(rc > 0 && s->sodeck_observe)
        s->sodeck_observe(s->sodeck_context, 0, 0, 6, (size_t)rc);
    return rc;
}

/* Deliberately separate send completion from response collection. No generic
 * user-supplied request, shell, process, or session.abstract callback is used. */
LIBSSH2_API int sodeck_probe_send(LIBSSH2_CHANNEL *c)
{
    static const char request[] = "sodeck-response@probe.invalid";
    unsigned char *p;
    int rc;
    if(c->sodeck_probe_state > 1 || c->local.close || c->remote.close)
        return LIBSSH2_ERROR_BAD_USE;
    if(!c->sodeck_probe_state) {
        if(c->process_state != libssh2_NB_state_idle)
            return LIBSSH2_ERROR_BAD_USE;
        p = c->sodeck_probe_packet;
        *(p++) = SSH_MSG_CHANNEL_REQUEST;
        _libssh2_store_u32(&p, c->remote.id);
        _libssh2_store_str(&p, request, sizeof(request) - 1);
        *(p++) = 1;
        c->sodeck_probe_length = p - c->sodeck_probe_packet;
        _libssh2_htonu32(c->sodeck_probe_channel, c->local.id);
        memset(&c->sodeck_probe_wait, 0, sizeof(c->sodeck_probe_wait));
        c->sodeck_probe_state = 1;
    }
    rc = _libssh2_transport_send(c->session, c->sodeck_probe_packet,
                                c->sodeck_probe_length, NULL, 0);
    if(!rc)
        c->sodeck_probe_state = 2;
    else if(rc != LIBSSH2_ERROR_EAGAIN)
        c->sodeck_probe_state = 3;
    return rc;
}

LIBSSH2_API int sodeck_probe_receive(LIBSSH2_CHANNEL *c)
{
    static const unsigned char replies[] = {
        SSH_MSG_CHANNEL_SUCCESS, SSH_MSG_CHANNEL_FAILURE, 0
    };
    unsigned char *data = NULL;
    size_t length = 0;
    int rc, reply;
    if(c->sodeck_probe_state != 2)
        return LIBSSH2_ERROR_BAD_USE;
    rc = _libssh2_packet_requirev(c->session, replies, &data, &length,
                                 1, c->sodeck_probe_channel, 4,
                                 &c->sodeck_probe_wait);
    if(rc == LIBSSH2_ERROR_EAGAIN)
        return rc;
    c->sodeck_probe_state = 3;
    if(rc)
        return rc;
    if(length != 5) {
        LIBSSH2_FREE(c->session, data);
        return LIBSSH2_ERROR_PROTO;
    }
    reply = data[0];
    LIBSSH2_FREE(c->session, data);
    c->sodeck_probe_state = 0;
    return reply;
}

LIBSSH2_API void sodeck_probe_cancel(LIBSSH2_CHANNEL *c)
{
    /* The owning I/O loop must finish a pending send before retiring/freeing
     * the channel; otherwise transport may still reference its packet. */
    if(c->sodeck_probe_state != 1)
        c->sodeck_probe_state = 3;
}
