#include <stdio.h>
#include <string.h>

#include "otio.h"

/* A clip with a blur and a dissolve into the next one, as an editor that
 * knows nothing of the registry writes them: named, and nothing more. */
static const char CUT[] =
    "{\"OTIO_SCHEMA\": \"Track.1\", \"name\": \"V1\", \"kind\": \"Video\","
    " \"children\": ["
    "  {\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"A\", \"effects\": ["
    "    {\"OTIO_SCHEMA\": \"Effect.1\", \"name\": \"blur\","
    "     \"effect_name\": \"Gaussian Blur\"}]},"
    "  {\"OTIO_SCHEMA\": \"Transition.2\", \"name\": \"dissolve\","
    "   \"transition_type\": \"SMPTE_Dissolve\","
    "   \"effect\": {\"OTIO_SCHEMA\": \"Effect.2\","
    "              \"effect_name\": \"Cross Dissolve\"}},"
    "  {\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"B\"}]}";

/* Prints what an effect was called and what the linker made of it. */
static void show(const OtioDocument *document, const char *holder, OtioNode effect) {
    OtioBuffer name, id;
    otio_effect_effect_name(document, effect, &name, NULL);
    otio_effect_effect_id(document, effect, &id, NULL);
    printf("%s: %s -> %s\n", holder, name.data, id.data);
    otio_buffer_free(name);
    otio_buffer_free(id);
}

int main(void) {
    /* otio_fx_registry is built in, but runs only when a read names it. */
    OtioReadOptions options;
    options.struct_size = sizeof options;
    otio_read_options_init(&options);
    options.effects_linker = "otio_fx_registry";

    OtioDocument *document = NULL;
    OtioBuffer error;
    if (otio_read_from_bytes(OTIO_FORMAT_OTIO_JSON, (const uint8_t *)CUT, strlen(CUT),
                             &options, &document, &error) != OTIO_STATUS_OK) {
        fprintf(stderr, "%s\n", error.data);
        otio_buffer_free(error);
        return 1;
    }

    OtioNode track, clip, transition, effect;
    otio_document_root(document, &track, NULL);

    otio_node_child_at(document, track, 0, &clip, NULL);
    otio_item_effect_at(document, clip, 0, &effect, NULL);
    show(document, "A", effect);

    /* A transition's effect is optional: OTIO_STATUS_NO_VALUE says it has none. */
    otio_node_child_at(document, track, 1, &transition, NULL);
    if (otio_transition_effect(document, transition, &effect, NULL) == OTIO_STATUS_OK) {
        show(document, "dissolve", effect);
    }

    otio_document_free(document);
    return 0;
}
