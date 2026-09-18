# Steps and attachments

A test case is more than a title. It carries the preconditions a tester needs, the ordered steps to
perform, the result the whole case expects, and — for most real tests — files: a screenshot, a log
excerpt, a sample payload. This page covers the two halves: the `steps` array inside the case
document, and the attachment routes that store files beside it.

For exact schemas, status codes and the multipart details of each route, use the Swagger UI at
`/api-docs` or [`openapi.json`](../../openapi.json). This page names routes and shows what lands on
disk.

## The case document

The fields that describe the test:

| Field | Type | Notes |
| --- | --- | --- |
| `title` | string | **Required.** The displayed name |
| `expectedResult` | string | **Required.** The outcome the case expects overall |
| `description` | string | Free prose |
| `preconditions` | string | What must be true before the first step |
| `steps` | array | Strings, or structured step objects (below) |
| `priority` | string | `Low`, `Medium`, `High` or `Critical` |
| `severity` | string | `Trivial`, `Minor`, `Major` or `Critical` |
| `testType` | string | Free-form classification, for example `Manual` |
| `exploratory` | boolean | Marks a session-based exploratory case |
| `attachments` | array | Files attached to the case as a whole |
| `tags` | array of strings | See [Tags and configurations](tags-and-configurations.md) |
| `version`, `lastModified` | API-managed | See [Case versioning and history](case-versioning-and-history.md) |

`testCaseId`, `title` and `expectedResult` are the required three; everything else is optional.

## Steps: simple or structured

The `steps` array accepts **either** form, element by element, so one case can mix them:

```json
{
  "steps": [
    "Open the login form",
    { "action": "Type the username", "expectedResult": "The cursor advances" },
    { "action": "Press Submit", "expectedResult": "An error is shown" }
  ]
}
```

A structured step is an object:

| Field | Type | Notes |
| --- | --- | --- |
| `action` | string | **Required.** What the tester does |
| `expectedResult` | string | What should happen after that action |
| `attachments` | array | Metadata for files attached to this one step (below) |

Use structured steps when each action has its own expected outcome and you want that outcome
attached to the right line; use plain strings for a quick list. `step_index` elsewhere in the API is
the **0-based position** in this array — `steps/0/attachments` belongs to `"Open the login form"`.

## Two kinds of attachment

| Kind | Routes | Stored in |
| --- | --- | --- |
| **Case attachment** | `POST` (`uploadTestCaseAttachment`), `GET …/{filename}` (`downloadTestCaseAttachment`), `DELETE …/{filename}` (`deleteTestCaseAttachment`) | the case folder, beside `test-case.json` |
| **Step attachment** | `POST` (`uploadStepAttachment`), `GET …/attachments` (`listStepAttachments`), `DELETE …/{filename}` (`deleteStepAttachment`) | the case folder's `steps/<index>/` |

Those are the **bare** routes, relative to `/test_cases/{id}`: they reach a case by its identifier
alone, which picks one occurrence only while that identifier has one home. Every one of them also
exists in two parent-scoped forms that name the folder holding the case, so an attachment of a case
id two parents share stays reachable:

| Addressed through | Case attachment | Step attachment |
| --- | --- | --- |
| the case id alone, `/test_cases/{id}/…` | `uploadTestCaseAttachment`, `downloadTestCaseAttachment`, `deleteTestCaseAttachment` | `uploadStepAttachment`, `listStepAttachments`, `deleteStepAttachment` |
| the holding project, `/projects/{id}/test_cases/{case_id}/…` | `uploadProjectTestCaseAttachment`, `downloadProjectTestCaseAttachment`, `deleteProjectTestCaseAttachment` | `uploadProjectTestCaseStepAttachment`, `listProjectTestCaseStepAttachments`, `deleteProjectTestCaseStepAttachment` |
| the holding suite, `/test_suites/{id}/test_cases/{case_id}/…` | `uploadTestSuiteTestCaseAttachment`, `downloadTestSuiteTestCaseAttachment`, `deleteTestSuiteTestCaseAttachment` | `uploadTestSuiteTestCaseStepAttachment`, `listTestSuiteTestCaseStepAttachments`, `deleteTestSuiteTestCaseStepAttachment` |

The path suffix is the same in every form — `/attachments` to upload, `/attachments/{filename}` to
download or delete, `/steps/{step_index}/attachments` to list or upload a step's files — so the
operation id is the only thing that changes. The bare form answers `409 conflict` when the case id
resolves to more than one folder; a parent-scoped form names the holder and does not have to guess.

```text
<case folder>/
├── test-case.json
├── revisions/v<n>.json
├── steps/
│   └── 0/                    attachments of the step at index 0
│       └── <uploaded files>
├── <case-level attachment files>
```

An uploaded file is stored under a sanitised name — a unique prefix, a dash, then the name the
client sent — and the case document records its metadata:

| Case attachment (`Attachment`) | Step attachment (`StepAttachment`) |
| --- | --- |
| `filename` — stored name | `filename` |
| `originalName` — name the client sent | `originalName` |
| `mimeType` — descriptive only | `mimeType` |
| `size` | `size` |
| `uploadedAt` — ISO-8601 UTC instant | — (not carried) |

`mimeType` is derived from the stored name and is metadata about the file: a download is always
answered as `application/octet-stream`, so the recorded type never becomes a response content type.

The upload response (`UploadResponse`) answers with `message`, `filename`, `originalName` and
`size`: **use the returned `filename`** in later calls rather than assuming the client's name
survived unchanged.

## Worked example

Starting from a running instance on `http://localhost:3100` (see
[Installation and first project](getting-started.md)), with the case from
[Projects, suites, and cases](projects-suites-and-cases.md) still present. Authentication is assumed
off; with it on, add `-H "Authorization: Bearer $TOKEN"`.

**1. Give the case structured steps and a case-level attachment list.**

```sh
curl -s -X PUT http://localhost:3100/test_cases/refund-partial.json \
  -H 'Content-Type: application/json' \
  -d '{
        "preconditions": "An order exists and is settled",
        "steps": [
          "Open the order in the back office",
          { "action": "Refund half the total", "expectedResult": "A confirmation dialog appears" },
          { "action": "Confirm", "expectedResult": "The order shows a partial refund" }
        ],
        "priority": "High",
        "severity": "Major"
      }'
```

```json
{"message":"Resource updated"}
```

`PUT` is a partial update: the fields you send replace the stored ones and the rest are kept. Steps
are replaced wholesale — you cannot append by sending one more element.

**2. Attach a file to the case as a whole.**

```sh
curl -s -X POST http://localhost:3100/test_cases/refund-partial.json/attachments \
  -F 'file=@./receipt.txt'
```

```json
{
  "message": "File uploaded successfully",
  "filename": "receipt.txt",
  "originalName": "receipt.txt",
  "size": 42
}
```

The multipart part name is **not** inspected — any name works — but the part must carry a filename.
A part without one is answered with `missing_file`. A body the multipart extractor cannot frame at
all (for example an upload sent as `application/json`) is answered with `400 text/plain`.

**3. Attach a file to a single step.** The index is 0-based, so index 2 is the step whose action is
"Confirm":

```sh
curl -s -X POST http://localhost:3100/test_cases/refund-partial.json/steps/2/attachments \
  -F 'file=@./confirmation.png'
```

```json
{
  "message": "File uploaded successfully",
  "filename": "confirmation.png",
  "originalName": "confirmation.png",
  "size": 2048
}
```

**4. List the step's attachments.**

| Route | Operation id | Returns |
| --- | --- | --- |
| `GET /test_cases/{id}/steps/{step_index}/attachments` | `listStepAttachments` | The step's `attachments` array |
| `GET /projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments` | `listProjectTestCaseStepAttachments` | The step's `attachments` array |
| `GET /test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments` | `listTestSuiteTestCaseStepAttachments` | The step's `attachments` array |

```sh
curl -s http://localhost:3100/test_cases/refund-partial.json/steps/2/attachments
```

```json
[{"filename":"confirmation.png","mimeType":"image/png","originalName":"confirmation.png","size":2048}]
```

**5. Download a case attachment.**

```sh
curl -s -o receipt.txt http://localhost:3100/test_cases/refund-partial.json/attachments/receipt.txt
```

`GET /test_cases/{id}/attachments/{filename}` (`downloadTestCaseAttachment`) answers the raw bytes
as `application/octet-stream`, with `Content-Disposition: attachment` naming the file the client
uploaded — an ASCII-safe `filename` plus an RFC 5987 `filename*` when the name is not plain ASCII.
The body is opaque whatever the file is: `mimeType` in the case document is metadata about the
stored file, and it never becomes the response content type, so a client that picks its decoder
from the response type still receives bytes and non-UTF-8 content survives unchanged. Case
attachments are also downloadable through that route's two parent-scoped mirrors —
`downloadProjectTestCaseAttachment` and `downloadTestSuiteTestCaseAttachment`, the same bytes under
the two paths above — and those three are the only download routes. **A step attachment has no
download route in any form** — only upload, list and delete — so treat the file under
`steps/<index>/` as write-only through the API.

There is likewise **no route that lists a case's attachments**, bare or parent-scoped: no
`GET /test_cases/{id}/attachments` exists, and the two parent-scoped mirrors above list *step*
attachments, not case ones. Read the case document instead, where the `attachments` array is stored:

```sh
curl -s http://localhost:3100/test_cases/refund-partial.json
```

```json
{"testCaseId":"refund-partial.json","title":"…","attachments":[{"filename":"receipt.txt","originalName":"receipt.txt","mimeType":"text/plain","size":42,"uploadedAt":"…"}]}
```

Step attachments *are* listable, because their route exists — in three forms, one per way of
addressing the case:

| Route | Operation id | Returns |
| --- | --- | --- |
| `GET /test_cases/{id}/steps/{step_index}/attachments` | `listStepAttachments` | The step's `attachments` array |
| `GET /projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments` | `listProjectTestCaseStepAttachments` | The step's `attachments` array |
| `GET /test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments` | `listTestSuiteTestCaseStepAttachments` | The step's `attachments` array |

**6. Delete.**

```sh
curl -s -X DELETE http://localhost:3100/test_cases/refund-partial.json/steps/2/attachments/confirmation.png
curl -s -X DELETE http://localhost:3100/test_cases/refund-partial.json/attachments/receipt.txt
```

`deleteTestCaseAttachment` and `deleteStepAttachment` each answer `200` with the message envelope.
Deleting the case removes its files — including everything under `steps/` and `revisions/` — because
they live in the case folder.

## Things that catch people out

| Symptom | Cause |
| --- | --- |
| `400` with code `invalid_request` naming `step_index` | The step index was not a non-negative integer (for example `steps/2a/attachments`) |
| `400` with code `invalid_request` | The step index is a valid integer but there is no step at that position |
| `404 not_found` | No case with that id (or a valid id that resolves to nothing); on a parent-scoped route, no case with that id under the named project or suite |
| `400 invalid_id` | The case identifier was rejected — a separator, `..`, or an absolute path in it. On a parent-scoped route the **parent** is read as a stored-document identifier too, so an unusable project or suite id answers `invalid_id` as well |
| `409 conflict` | A bare attachment route was given a case id that resolves to more than one folder. Use the parent-scoped form, which names the holder. A suite-scoped route still answers `409` when the *suite* id resolves to more than one project |
| `413` with the plain-text body `length limit exceeded` | The body exceeded 50 MiB. This is the router's limit, checked before any handler, so it answers plain text, not the envelope |
| `400 invalid_request` naming `attachments` | An attachment filename was not a plain path segment (a separator, `..`, or an absolute path) |
| `400 missing_file` | The multipart part carried no filename |
| A step attachment you expected is missing after a `PUT` | A `PUT` replaces the whole `steps` array. Re-send the attachment metadata with the step, or upload the file again |

Two more rules worth knowing:

- **There is no route that attaches a step file by uploading and indexing it in one call.** You
  upload to `steps/{step_index}/attachments` and the stored file's metadata appears in the case
  document's step entry. A `PUT` that does not carry the metadata replaces the step and drops the
  reference to the file, so upload *after* the last edit to that step, or include the metadata in the
  `PUT`.
- **A step attachment is not a test result attachment.** Files recorded against an execution live in
  the run's result, not the case. See [Test runs and results](test-runs-and-results.md).

## Next

| I want to… | Read |
| --- | --- |
| Understand what a case's folder looks like next to its parents | [Projects, suites, and cases](projects-suites-and-cases.md) |
| Carry a case's steps, attachments and revision history to another parent | [Composing and duplicating](composing-and-duplicating.md) — composition `copy` duplicates the folder; `duplicate` copies the document only |
| See how a step edit records a revision | [Case versioning and history](case-versioning-and-history.md) |
| Record the outcome of executing a case | [Test runs and results](test-runs-and-results.md) |

---

*Sources of truth: [`openapi.json`](../../openapi.json) for every attachment and step route,
parameter and schema named here; the storage concept in the
[repository README](../../README.md#storage-concept) for the on-disk layout; the
[compatibility contract](../contracts/api-compatibility.md) for the per-step attachment plan (#93)
and the parent-scoped attachment routes (#290).
Where this page and one of those disagree, the source wins and this page is a bug.*
