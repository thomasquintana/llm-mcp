# Base64 images, embeddings and audio

All image/audio bytes are supplied inline as base64. The server does not download
media URLs or read caller-provided local file paths. OpenAI validates the decoded
file format and model compatibility. Models remain caller-selected.

## Images

Use Responses/Decisions content parts:

```json
{"type":"input_image","image_url":"data:image/png;base64,<canonical-base64>"}
```

Supported image MIME types are `image/png`, `image/jpeg`, `image/webp`, and
`image/gif`. `detail` and other provider image options are preserved. HTTP(S) image
URLs, image file IDs, local references, malformed/empty base64 and unsupported MIME
types fail before provider dispatch. The same validation covers user-message
content, function/custom tool-output evidence and Decisions question instructions.
Unrelated JSON and metadata are preserved without interpreting them as media.

`generate` and `classify` use `input_text`/`input_image` parts, not Chat Completions'
`text`/`image_url` part types. Supply a model that accepts images. OAuth generation
preserves base64 image input through the internal streaming adapter.

## Contextual image embeddings

`embed` accepts either standard OpenAI `input` (a string, string batch, token array,
or token batch) or a nonempty `inputs` batch. These fields are mutually exclusive.
Each multimodal item has only `content`, an ordered nonempty array of `input_text`
and/or base64 `input_image` parts. Text-only items must contain nonempty text.

Image-bearing items require `description.model`. Optional
`description.instructions` supplements the built-in detailed retrieval description
instructions. Other fields inside `description` are rejected. Embedding options,
including `dimensions`, `encoding_format` and unknown provider fields, are forwarded
to `/embeddings`; preparation-only fields and false execution flags are removed.

For each image-bearing item, `/responses` considers all its images jointly and
describes visible objects, relationships, layout, text, labels and distinguishing
details using the accompanying text as context. It is instructed to distinguish
uncertainty and avoid unseen details. Original text parts retain their content and
order, joined by two newlines. With context, the exact embedding text is:

```text
Context:
<original text context>

Image description:
<generated detailed description>
```

Without context, only the description is embedded. Text-only items embed their
joined text directly. Each item yields one vector, even if it has multiple images.
Use separate items for independent vectors. All preparation must succeed before
one final ordered embedding batch; no partial results or automatic retries are
returned. An error identifies the preparation `input_index` when available. Earlier
description calls may already have incurred usage. Retrying can generate different
descriptions; retain returned preparation text when reproducibility is needed.

The result envelope is:

```json
{
  "response": {"data": [], "model": "<embedding-model>", "usage": {}},
  "prepared_inputs": [
    {
      "index": 0,
      "text": "<exact submitted context plus description>",
      "description": "<generated description>",
      "description_model": "<vision-model>",
      "description_usage": {}
    }
  ]
}
```

`response` retains the full embedding provider object. Usage in that object is for
embedding; each image item's `description_usage` is separate and is included when
returned by OpenAI. Prepared text-only items have only `index` and `text`. Standard
`input` requests return `prepared_inputs:[]` because no preparation occurred.

## Audio transcription

`transcribe` requires `model` and
`file:{filename,mime_type,data_base64}`. Supply raw canonical base64 in
`data_base64`, without a data URL prefix. `filename` is a basename with a supported
extension, not a server path. Supported extensions: `flac`, `mp3`, `mp4`, `mpeg`,
`mpga`, `m4a`, `ogg`, `wav`, and `webm`.

Supported MIME types: `audio/flac`, `audio/x-flac`, `audio/mpeg`, `audio/mp3`,
`audio/mp4`, `audio/m4a`, `audio/x-m4a`, `audio/ogg`, `audio/wav`, `audio/x-wav`,
`audio/wave`, `audio/webm`, `video/mp4`, `video/mpeg`, and `video/webm`.

The server constructs a multipart `/audio/transcriptions` upload preserving file
bytes, filename and MIME type. Optional scalar fields become text fields, scalar
arrays become repeated `name[]` fields and nested objects use `name[property]`.
Multipart option keys use letters, digits and underscores. Unknown options are
forwarded using these conventions; OpenAI validates model-specific combinations.

For example, a diarized request can select `gpt-4o-transcribe-diarize`,
`response_format:"diarized_json"` and `chunking_strategy:"auto"`. Supply
`known_speaker_names` and corresponding `known_speaker_references` arrays for known
speakers. References must be base64 audio data URLs, such as
`data:audio/wav;base64,<encoded-sample>`, with supported audio MIME types. Their
decoded sizes count toward the same media bound. OpenAI validates sample durations.

`response_format` defaults to `json`. Full `json`, `verbose_json` and
`diarized_json` objects retain usage, timestamps, speaker annotations, log
probabilities and unknown fields. `text`, `srt` and `vtt` return
`{text, response_format}` without altering whitespace or subtitle contents. Format
support, timestamp options and log probabilities depend on the selected model.

## Authentication and resource bounds

`LLM_MCP_AUTH=api_key` enables all four tools and requires `OPENAI_API_KEY`.
`LLM_MCP_AUTH=oauth` enables only `generate` and conflicts with a supplied API key.
The other tools are omitted from discovery/catalogs and direct calls fail with
`tool_disabled` before provider activity. Neither authentication fallback nor
mixed-authentication pipelines are used.

`LLM_MCP_MAX_REQUEST_BYTES` bounds encoded arguments, aggregate decoded media and
each outbound provider request, including transcription multipart overhead.
Base64 expands bytes by roughly 4/3, so the 8 MiB default argument limit permits
approximately 6 MiB of raw media, less metadata. Increase it for larger recordings
within OpenAI's model/upload limits. No automatic audio splitting is performed.

`LLM_MCP_MAX_RESPONSE_BYTES` bounds each provider response and the assembled result,
including embedding descriptions and text/subtitle wrappers. Oversized preparations
fail instead of silently truncating. One tool deadline covers queueing, decoding,
all description calls, embedding/upload and result assembly. A pipeline holds one
concurrency permit, so concurrency one is supported. Cancellation stops local I/O
and subsequent work, but cannot undo provider work already accepted.
