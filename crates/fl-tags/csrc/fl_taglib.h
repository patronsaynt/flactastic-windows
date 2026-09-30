#ifndef FL_TAGLIB_H
#define FL_TAGLIB_H

/*
 * C shim over TagLib for the desktop port.
 *
 * Every tag operation goes through the same TagLib calls the macOS app uses
 * (the TagLib C API plus a port of Sources/CTagLibHelper/taglib_helpers.cpp),
 * so what lands on disk is identical. Differences, all transport-level:
 *   - paths are UTF-8 and opened with wide APIs on Windows;
 *   - C-API string management is disabled; strings returned here are
 *     heap-allocated and released with fl_tl_free (thread-safe);
 *   - audio properties are exposed for the scanner.
 */

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FlTlFile FlTlFile;

/* Opens `utf8_path`. Returns NULL when the file can't be opened or TagLib
 * doesn't consider it valid. */
FlTlFile *fl_tl_open(const char *utf8_path);
void fl_tl_close(FlTlFile *f);
/* Non-zero on success. */
int fl_tl_save(FlTlFile *f);
void fl_tl_free(void *p);

/* Basic tag. Returned strings are UTF-8, heap-allocated (fl_tl_free), and
 * NULL when the file has no tag. `which`: 0 title, 1 artist, 2 album,
 * 3 genre, 4 comment. */
char *fl_tl_tag_get(FlTlFile *f, int which);
void fl_tl_tag_set(FlTlFile *f, int which, const char *utf8);
unsigned int fl_tl_tag_year(FlTlFile *f);
unsigned int fl_tl_tag_track(FlTlFile *f);
void fl_tl_tag_set_year(FlTlFile *f, unsigned int year);
void fl_tl_tag_set_track(FlTlFile *f, unsigned int track);

/* Generic property API: first value of `key`, or NULL when unset/empty. */
char *fl_tl_property_get(FlTlFile *f, const char *key);
/* Empty string clears the property. */
void fl_tl_property_set(FlTlFile *f, const char *key, const char *utf8);

/* Pictures (port of the taglib_helper_* picture functions). */
int fl_tl_set_picture(FlTlFile *f, const char *data, unsigned int size, const char *mime);
void fl_tl_remove_pictures(FlTlFile *f);
unsigned char *fl_tl_read_picture(FlTlFile *f, unsigned int *out_size);

/* Audio properties from an already-open file. Any field may be 0. */
typedef struct {
    int length_ms;
    int sample_rate;
    int channels;
    int bitrate_kbps;
    int bits_per_sample; /* FLAC / MP4 / WAV / AIFF only */
} FlTlAudio;
int fl_tl_audio(FlTlFile *f, FlTlAudio *out);

/* taglib_helper_bits_per_sample: opens with Average-accuracy properties. */
int fl_tl_bits_per_sample(const char *utf8_path);

#ifdef __cplusplus
}
#endif

#endif
