#include <stdio.h>
#include <string.h>

#include "otio.h"

/* A cut of two clips, as an .otio file would hold it. */
static const char CUT[] =
    "{\"OTIO_SCHEMA\": \"Track.1\", \"name\": \"V1\", \"kind\": \"Video\","
    " \"children\": [{\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"A\"},"
    "                {\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"B\"}]}";

/* A media linker is handed each clip as it is read, and an object whose
 * metadata is the arguments the read was given. It answers with the media
 * the clip should use, made in the same document, or leaves out_result
 * alone to leave the clip as it was. */
static OtioStatus link_proxy(void *context, OtioDocument *document, OtioNode clip,
                             OtioNode arguments, OtioNode *out_result,
                             char *message, size_t message_capacity) {
    OtioBuffer name, root;
    char url[512];
    (void)context;
    if (otio_node_name(document, clip, &name, NULL) != OTIO_STATUS_OK) {
        return OTIO_STATUS_INVALID_ARGUMENT;
    }
    if (otio_metadata_get_string(document, arguments, "root", &root, NULL) != OTIO_STATUS_OK) {
        /* Whatever goes in message is the failure the read reports. */
        snprintf(message, message_capacity, "no root to link under");
        otio_buffer_free(name);
        return OTIO_STATUS_INVALID_ARGUMENT;
    }
    snprintf(url, sizeof url, "%s/%s.mov", root.data, name.data);
    otio_buffer_free(root);
    OtioStatus status = otio_external_reference_new(document, name.data, url, out_result, NULL);
    otio_buffer_free(name);
    return status;
}

/* A hook script is handed the whole result, and answers with what the read
 * goes on with: here the same object, stamped. The context is whatever was
 * registered with it, here the key to stamp under. */
static OtioStatus stamp(void *context, OtioDocument *document, OtioNode target,
                        OtioNode arguments, OtioNode *out_result,
                        char *message, size_t message_capacity) {
    OtioBuffer who;
    (void)message;
    (void)message_capacity;
    if (otio_metadata_get_string(document, arguments, "who", &who, NULL) != OTIO_STATUS_OK) {
        return OTIO_STATUS_INVALID_ARGUMENT;
    }
    OtioStatus status = otio_metadata_set_string(document, target, (const char *)context, who.data, NULL);
    otio_buffer_free(who);
    *out_result = target;
    return status;
}

int main(void) {
    /* The last two arguments are a context handed to every call, and a
     * function the library calls to release it when the plugin goes. */
    otio_register_media_linker("proxies", link_proxy, NULL, NULL, NULL);
    otio_register_hook_script("stamp", stamp, "read_by", NULL, NULL);
    otio_attach_hook_script("post_adapter_read", "stamp", NULL);

    /* The read names the linker, and carries both sets of arguments as JSON. */
    OtioReadOptions options = otio_read_options_default();
    options.media_linker = "proxies";
    options.media_linker_arguments = "{\"root\": \"/proxies\"}";
    options.hook_arguments = "{\"who\": \"the conform\"}";

    OtioDocument *document = NULL;
    OtioBuffer error;
    if (otio_read_from_bytes(OTIO_FORMAT_OTIO_JSON, (const uint8_t *)CUT, strlen(CUT),
                             &options, &document, &error) != OTIO_STATUS_OK) {
        fprintf(stderr, "%s\n", error.data);
        otio_buffer_free(error);
        return 1;
    }

    OtioNode track, clips[2], media;
    OtioBuffer text;
    size_t count = 0;
    otio_document_root(document, &track, NULL);
    otio_metadata_get_string(document, track, "read_by", &text, NULL);
    printf("read by %s\n", text.data);
    otio_buffer_free(text);

    otio_node_find_clips(document, track, clips, 2, &count, NULL);
    for (size_t i = 0; i < count; i++) {
        OtioBuffer name;
        otio_node_name(document, clips[i], &name, NULL);
        otio_clip_media_reference(document, clips[i], NULL, &media, NULL);
        otio_external_reference_target_url(document, media, &text, NULL);
        printf("%s -> %s\n", name.data, text.data);
        otio_buffer_free(name);
        otio_buffer_free(text);
    }

    otio_document_free(document);
    otio_detach_hook_script("post_adapter_read", "stamp");
    otio_unregister_hook_script("stamp");
    otio_unregister_media_linker("proxies");
    return 0;
}
