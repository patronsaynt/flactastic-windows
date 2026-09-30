// Port of Sources/CTagLibHelper/taglib_helpers.cpp plus thin wrappers over
// the TagLib C API. Tag reads/writes call the same C API functions as the
// macOS app so the on-disk result is identical.

#include "fl_taglib.h"

#include <taglib/tag_c.h>
#include <taglib/fileref.h>
#include <taglib/audioproperties.h>
#include <taglib/flacproperties.h>
#include <taglib/mp4properties.h>
#include <taglib/wavproperties.h>
#include <taglib/aiffproperties.h>

#include <cstdlib>
#include <cstring>
#include <mutex>
#include <string>

#ifdef _WIN32
#include <windows.h>
#endif

namespace {

std::once_flag g_init;

void init_once() {
    std::call_once(g_init, [] {
        // Returned strings are owned by the caller (freed with fl_tl_free),
        // so TagLib's global, non-thread-safe string list is never used.
        taglib_set_string_management_enabled(0);
        taglib_set_strings_unicode(1);
    });
}

TagLib_File *as_c(FlTlFile *f) { return reinterpret_cast<TagLib_File *>(f); }

#ifdef _WIN32
std::wstring widen(const char *utf8) {
    int n = MultiByteToWideChar(CP_UTF8, 0, utf8, -1, nullptr, 0);
    if (n <= 0) return std::wstring();
    std::wstring w(static_cast<size_t>(n), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, utf8, -1, &w[0], n);
    w.resize(static_cast<size_t>(n - 1));
    return w;
}
#endif

char *dup_first(char **values) {
    char *result = nullptr;
    if (values[0] && values[0][0] != '\0') {
        size_t n = strlen(values[0]);
        result = static_cast<char *>(malloc(n + 1));
        if (result) memcpy(result, values[0], n + 1);
    }
    taglib_property_free(values);
    return result;
}

}  // namespace

extern "C" {

FlTlFile *fl_tl_open(const char *utf8_path) {
    init_once();
    if (!utf8_path) return nullptr;
#ifdef _WIN32
    std::wstring w = widen(utf8_path);
    TagLib_File *f = taglib_file_new_wchar(w.c_str());
#else
    TagLib_File *f = taglib_file_new(utf8_path);
#endif
    if (!f) return nullptr;
    if (taglib_file_is_valid(f) == 0) {
        taglib_file_free(f);
        return nullptr;
    }
    return reinterpret_cast<FlTlFile *>(f);
}

void fl_tl_close(FlTlFile *f) {
    if (f) taglib_file_free(as_c(f));
}

int fl_tl_save(FlTlFile *f) { return f && taglib_file_save(as_c(f)) != 0; }

void fl_tl_free(void *p) { taglib_free(p); }

char *fl_tl_tag_get(FlTlFile *f, int which) {
    if (!f) return nullptr;
    TagLib_Tag *tag = taglib_file_tag(as_c(f));
    if (!tag) return nullptr;
    switch (which) {
    case 0: return taglib_tag_title(tag);
    case 1: return taglib_tag_artist(tag);
    case 2: return taglib_tag_album(tag);
    case 3: return taglib_tag_genre(tag);
    case 4: return taglib_tag_comment(tag);
    default: return nullptr;
    }
}

void fl_tl_tag_set(FlTlFile *f, int which, const char *utf8) {
    if (!f || !utf8) return;
    TagLib_Tag *tag = taglib_file_tag(as_c(f));
    if (!tag) return;
    switch (which) {
    case 0: taglib_tag_set_title(tag, utf8); break;
    case 1: taglib_tag_set_artist(tag, utf8); break;
    case 2: taglib_tag_set_album(tag, utf8); break;
    case 3: taglib_tag_set_genre(tag, utf8); break;
    case 4: taglib_tag_set_comment(tag, utf8); break;
    default: break;
    }
}

unsigned int fl_tl_tag_year(FlTlFile *f) {
    TagLib_Tag *tag = f ? taglib_file_tag(as_c(f)) : nullptr;
    return tag ? taglib_tag_year(tag) : 0;
}

unsigned int fl_tl_tag_track(FlTlFile *f) {
    TagLib_Tag *tag = f ? taglib_file_tag(as_c(f)) : nullptr;
    return tag ? taglib_tag_track(tag) : 0;
}

void fl_tl_tag_set_year(FlTlFile *f, unsigned int year) {
    TagLib_Tag *tag = f ? taglib_file_tag(as_c(f)) : nullptr;
    if (tag) taglib_tag_set_year(tag, year);
}

void fl_tl_tag_set_track(FlTlFile *f, unsigned int track) {
    TagLib_Tag *tag = f ? taglib_file_tag(as_c(f)) : nullptr;
    if (tag) taglib_tag_set_track(tag, track);
}

// taglib_helper_get_album_artist / _get_lyrics / _get_cuesheet all share this shape.
char *fl_tl_property_get(FlTlFile *f, const char *key) {
    if (!f || !key) return nullptr;
    char **values = taglib_property_get(as_c(f), key);
    if (!values) return nullptr;
    return dup_first(values);
}

// taglib_helper_set_* : an empty string clears the property cross-format.
void fl_tl_property_set(FlTlFile *f, const char *key, const char *utf8) {
    if (!f || !key || !utf8) return;
    taglib_property_set(as_c(f), key, utf8);
}

int fl_tl_set_picture(FlTlFile *f, const char *data, unsigned int size, const char *mime) {
    if (!f || !data || size == 0 || !mime) return 0;
    TAGLIB_COMPLEX_PROPERTY_PICTURE(props, data, size, "", mime, "Front Cover");
    return taglib_complex_property_set(as_c(f), "PICTURE", props);
}

void fl_tl_remove_pictures(FlTlFile *f) {
    if (!f) return;
    taglib_complex_property_set(as_c(f), "PICTURE", NULL);
}

unsigned char *fl_tl_read_picture(FlTlFile *f, unsigned int *out_size) {
    if (!f || !out_size) return nullptr;
    *out_size = 0;
    TagLib_Complex_Property_Attribute ***props = taglib_complex_property_get(as_c(f), "PICTURE");
    if (!props) return nullptr;
    unsigned char *result = nullptr;
    for (int i = 0; props[i] != NULL; i++) {
        for (int j = 0; props[i][j] != NULL; j++) {
            TagLib_Complex_Property_Attribute *attr = props[i][j];
            if (strcmp(attr->key, "data") == 0 && attr->value.type == TagLib_Variant_ByteVector &&
                attr->value.size > 0) {
                result = static_cast<unsigned char *>(malloc(attr->value.size));
                if (result) {
                    memcpy(result, attr->value.value.byteVectorValue, attr->value.size);
                    *out_size = attr->value.size;
                }
                taglib_complex_property_free(props);
                return result;
            }
        }
    }
    taglib_complex_property_free(props);
    return nullptr;
}

static int bits_of(const TagLib::AudioProperties *props) {
    if (auto p = dynamic_cast<const TagLib::FLAC::Properties *>(props)) return p->bitsPerSample();
    if (auto p = dynamic_cast<const TagLib::MP4::Properties *>(props)) return p->bitsPerSample();
    if (auto p = dynamic_cast<const TagLib::RIFF::WAV::Properties *>(props)) return p->bitsPerSample();
    if (auto p = dynamic_cast<const TagLib::RIFF::AIFF::Properties *>(props)) return p->bitsPerSample();
    return 0;
}

int fl_tl_audio(FlTlFile *f, FlTlAudio *out) {
    if (!f || !out) return 0;
    memset(out, 0, sizeof(*out));
    auto ref = reinterpret_cast<TagLib::FileRef *>(f);
    const TagLib::AudioProperties *props = ref->audioProperties();
    if (!props) return 0;
    out->length_ms = props->lengthInMilliseconds();
    out->sample_rate = props->sampleRate();
    out->channels = props->channels();
    out->bitrate_kbps = props->bitrate();
    out->bits_per_sample = bits_of(props);
    return 1;
}

int fl_tl_bits_per_sample(const char *utf8_path) {
    init_once();
    if (!utf8_path) return 0;
#ifdef _WIN32
    std::wstring w = widen(utf8_path);
    TagLib::FileRef f(w.c_str(), true, TagLib::AudioProperties::Average);
#else
    TagLib::FileRef f(utf8_path, true, TagLib::AudioProperties::Average);
#endif
    if (f.isNull()) return 0;
    const TagLib::AudioProperties *props = f.audioProperties();
    return props ? bits_of(props) : 0;
}

}  // extern "C"
