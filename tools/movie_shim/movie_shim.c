// tools/movie_shim/movie_shim.c - demuxing and decoding for the engine's movie:: HLE (crates/engine/src/movie.rs),
// on top of ffmpeg (libavformat / libavcodec / libswscale / libswresample). Built by tools/build_movie_shim.sh into
// target/movie_shim.dylib, which the engine dlopens; without it movies fail cleanly.
//
// The game drives a MediaCodec-style API; this file only turns a file into
// packets and packets into frames / PCM.
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/imgutils.h>
#include <libswresample/swresample.h>
#include <libswscale/swscale.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    int type;          // 1 video, 2 audio, 0 other
    int codec;         // movie::DecoderType: 1 AVC, 2 VP8, 3 VP9, 4 AAC, 5 Vorbis, 0 unknown
    int width, height;
    double fps;
    int sample_rate, channels;
    int64_t duration_us;
} ms_track;

typedef struct {
    AVFormatContext *fmt;
    AVPacket *pkt;
    int have_pkt;      // pkt holds the current sample
    int eof;
} ms_file;

static int codec_kind(enum AVCodecID id) {
    switch (id) {
    case AV_CODEC_ID_H264: return 1;
    case AV_CODEC_ID_VP8: return 2;
    case AV_CODEC_ID_VP9: return 3;
    case AV_CODEC_ID_AAC: return 4;
    case AV_CODEC_ID_VORBIS: return 5;
    case AV_CODEC_ID_OPUS: return 6;
    default: return 0;
    }
}

void *ms_open(const char *path, int *ntracks) {
    ms_file *f = calloc(1, sizeof(ms_file));
    if (avformat_open_input(&f->fmt, path, NULL, NULL) < 0) { free(f); return NULL; }
    if (avformat_find_stream_info(f->fmt, NULL) < 0) { avformat_close_input(&f->fmt); free(f); return NULL; }
    f->pkt = av_packet_alloc();
    *ntracks = (int)f->fmt->nb_streams;
    return f;
}

int ms_track_info(void *h, int idx, ms_track *out) {
    ms_file *f = h;
    if (idx < 0 || idx >= (int)f->fmt->nb_streams) return -1;
    AVStream *s = f->fmt->streams[idx];
    AVCodecParameters *p = s->codecpar;
    memset(out, 0, sizeof(*out));
    out->type = p->codec_type == AVMEDIA_TYPE_VIDEO ? 1 : p->codec_type == AVMEDIA_TYPE_AUDIO ? 2 : 0;
    out->codec = codec_kind(p->codec_id);
    out->width = p->width;
    out->height = p->height;
    out->fps = s->avg_frame_rate.den ? av_q2d(s->avg_frame_rate) : 0;
    out->sample_rate = p->sample_rate;
    out->channels = p->ch_layout.nb_channels;
    int64_t d = s->duration != AV_NOPTS_VALUE ? av_rescale_q(s->duration, s->time_base, AV_TIME_BASE_Q) : f->fmt->duration;
    out->duration_us = d > 0 ? d : 0;
    return 0;
}

// The current sample (reads the next packet when none is held): track index, size, time, keyframe flag.
// Returns 0, or -1 at end of file.
int ms_peek(void *h, int *track, int *size, int64_t *pts_us, int *key) {
    ms_file *f = h;
    if (!f->have_pkt) {
        if (f->eof) return -1;
        av_packet_unref(f->pkt);
        if (av_read_frame(f->fmt, f->pkt) < 0) { f->eof = 1; return -1; }
        f->have_pkt = 1;
    }
    AVStream *s = f->fmt->streams[f->pkt->stream_index];
    *track = f->pkt->stream_index;
    *size = f->pkt->size;
    int64_t t = f->pkt->pts != AV_NOPTS_VALUE ? f->pkt->pts : f->pkt->dts;
    *pts_us = t != AV_NOPTS_VALUE ? av_rescale_q(t, s->time_base, AV_TIME_BASE_Q) : 0;
    *key = (f->pkt->flags & AV_PKT_FLAG_KEY) != 0;
    return 0;
}

// Copies the current sample into buf (cap bytes). Returns its size, -1 at end of file, -2 if it does not fit.
int ms_copy(void *h, uint8_t *buf, int cap) {
    ms_file *f = h;
    int track, size, key;
    int64_t pts;
    if (ms_peek(h, &track, &size, &pts, &key) < 0) return -1;
    if (size > cap) return -2;
    memcpy(buf, f->pkt->data, size);
    return size;
}

void ms_advance(void *h) { ((ms_file *)h)->have_pkt = 0; }

int ms_seek(void *h, int64_t us) {
    ms_file *f = h;
    f->have_pkt = 0;
    f->eof = 0;
    return av_seek_frame(f->fmt, -1, av_rescale(us, AV_TIME_BASE, 1000000), AVSEEK_FLAG_BACKWARD);
}

void ms_close(void *h) {
    ms_file *f = h;
    if (!f) return;
    av_packet_free(&f->pkt);
    avformat_close_input(&f->fmt);
    free(f);
}

// ── decoder ──────────────────────────────────────────────────────────────────────────────────────────────────────
// out_format: movie::DecoderOutputFormat: 1 PCM16, 3 ABGR (bytes R,G,B,A), 4 NV12.
typedef struct {
    AVCodecContext *ctx;
    AVFrame *frame;
    AVPacket *pkt;
    struct SwsContext *sws;
    SwrContext *swr;
    int out_format;
    int pending;       // a frame was received and not yet returned
    int flushed;
} md_dec;

void *md_open(void *file, int track, int out_format) {
    ms_file *f = file;
    if (track < 0 || track >= (int)f->fmt->nb_streams) return NULL;
    AVCodecParameters *p = f->fmt->streams[track]->codecpar;
    const AVCodec *c = avcodec_find_decoder(p->codec_id);
    if (!c) return NULL;
    md_dec *d = calloc(1, sizeof(md_dec));
    d->ctx = avcodec_alloc_context3(c);
    avcodec_parameters_to_context(d->ctx, p);
    d->ctx->thread_count = 0;  // automatic
    // Slice threading only: frame threading delays output by a frame per thread, and players feed only a few inputs
    // (as many as a hardware decoder needs: its reorder depth) before waiting for the first output.
    d->ctx->thread_type = FF_THREAD_SLICE;
    d->ctx->pkt_timebase = f->fmt->streams[track]->time_base;
    if (avcodec_open2(d->ctx, c, NULL) < 0) { avcodec_free_context(&d->ctx); free(d); return NULL; }
    d->frame = av_frame_alloc();
    d->pkt = av_packet_alloc();
    d->out_format = out_format;
    return d;
}

// Feeds one compressed sample (pts in microseconds); eos drains the decoder. Returns 0, or <0 on error.
int md_send(void *h, const uint8_t *data, int size, int64_t pts_us, int eos) {
    md_dec *d = h;
    int r = 0;
    if (size > 0) {
        av_packet_unref(d->pkt);
        if (av_new_packet(d->pkt, size) < 0) return -1;
        memcpy(d->pkt->data, data, size);
        d->pkt->pts = av_rescale_q(pts_us, AV_TIME_BASE_Q, d->ctx->pkt_timebase);
        r = avcodec_send_packet(d->ctx, d->pkt);
        if (r == AVERROR(EAGAIN)) r = 0;  // output must be drained first; the caller receives, then resends
    }
    if (eos && !d->flushed) {
        avcodec_send_packet(d->ctx, NULL);
        d->flushed = 1;
    }
    return r < 0 && r != AVERROR_EOF ? r : 0;
}

// Size in bytes of one output for the current stream (call after a frame is available).
static int out_size(md_dec *d, int w, int h) {
    if (d->out_format == 4) return w * h + 2 * ((w + 1) / 2) * ((h + 1) / 2);
    if (d->out_format == 3) return w * h * 4;
    return 0;
}

// Next decoded output into out (cap bytes). Returns its size, 0 when the decoder needs input, -1 at end of stream,
// -2 if out is too small (*need set). Video: *w, *h, *stride; audio: *w = sample frames, *h = channels.
int md_receive(void *h, uint8_t *out, int cap, int64_t *pts_us, int *w, int *hh, int *stride, int *need) {
    md_dec *d = h;
    if (!d->pending) {
        int r = avcodec_receive_frame(d->ctx, d->frame);
        if (r == AVERROR(EAGAIN)) return 0;
        if (r == AVERROR_EOF) return -1;
        if (r < 0) return -1;
        d->pending = 1;
    }
    AVFrame *fr = d->frame;
    int64_t t = fr->best_effort_timestamp != AV_NOPTS_VALUE ? fr->best_effort_timestamp : fr->pts;
    *pts_us = t != AV_NOPTS_VALUE ? av_rescale_q(t, d->ctx->pkt_timebase, AV_TIME_BASE_Q) : 0;
    if (d->ctx->codec_type == AVMEDIA_TYPE_VIDEO) {
        int W = fr->width, H = fr->height, size = out_size(d, W, H);
        *w = W; *hh = H; *stride = W;
        if (size > cap) { *need = size; return -2; }
        if (d->out_format == 4) {
            d->sws = sws_getCachedContext(d->sws, W, H, fr->format, W, H, AV_PIX_FMT_NV12, SWS_BILINEAR, NULL, NULL, NULL);
            uint8_t *dst[4] = { out, out + W * H, NULL, NULL };
            int ls[4] = { W, W, 0, 0 };
            sws_scale(d->sws, (const uint8_t *const *)fr->data, fr->linesize, 0, H, dst, ls);
        } else {
            d->sws = sws_getCachedContext(d->sws, W, H, fr->format, W, H, AV_PIX_FMT_RGBA, SWS_BILINEAR, NULL, NULL, NULL);
            uint8_t *dst[4] = { out, NULL, NULL, NULL };
            int ls[4] = { W * 4, 0, 0, 0 };
            sws_scale(d->sws, (const uint8_t *const *)fr->data, fr->linesize, 0, H, dst, ls);
            *stride = W * 4;
        }
        d->pending = 0;
        av_frame_unref(fr);
        return size;
    }
    // Audio -> interleaved signed 16-bit at the source rate and channel count.
    int ch = fr->ch_layout.nb_channels, n = fr->nb_samples, size = n * ch * 2;
    *w = n; *hh = ch; *stride = 0;
    if (size > cap) { *need = size; return -2; }
    if (!d->swr) {
        swr_alloc_set_opts2(&d->swr, &fr->ch_layout, AV_SAMPLE_FMT_S16, fr->sample_rate, &fr->ch_layout, fr->format,
                            fr->sample_rate, 0, NULL);
        swr_init(d->swr);
    }
    uint8_t *dst[1] = { out };
    int got = swr_convert(d->swr, dst, n, (const uint8_t **)fr->extended_data, n);
    d->pending = 0;
    av_frame_unref(fr);
    return got < 0 ? -1 : got * ch * 2;
}

void md_flush(void *h) {
    md_dec *d = h;
    avcodec_flush_buffers(d->ctx);
    d->pending = 0;
    d->flushed = 0;
}

void md_close(void *h) {
    md_dec *d = h;
    if (!d) return;
    sws_freeContext(d->sws);
    swr_free(&d->swr);
    av_packet_free(&d->pkt);
    av_frame_free(&d->frame);
    avcodec_free_context(&d->ctx);
    free(d);
}
