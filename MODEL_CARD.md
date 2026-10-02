# LiveBlock detector model card

## Installed model: AdSERP starter, October 1, 2026

The signed Mac release currently contains a YOLOv8n model fine-tuned on public
search-ad screenshots. Twelve full training epochs completed; training was
interrupted during epoch 13 because of machine memory pressure. The best
completed checkpoint was retained and independently evaluated before export.

Source: [AdSERP](https://zenodo.org/records/15236546), CC BY 4.0.
Kayhan Latifzadeh, Jacek Gwizdka, Luis A. Leiva. *A Versatile Dataset of Mouse and
Eye Movements on Search Engine Results Pages* (SIGIR 2025), DOI
10.1145/3726302.3730325. Changes: one page per query, viewport crops, exact crop
deduplication, grouped splits and normalized YOLO ad-boundary labels.

The dataset has 2,494 training, 571 validation and 573 test images. Test includes
949 annotated ads and 127 images without ads. No query crosses splits.

| Deployment path | True positives | False positives | Missed ads | Precision | Recall |
|---|---:|---:|---:|---:|---:|
| Mac Vision/CoreML | 941 | 3 | 8 | 99.68% | 99.16% |
| Windows detector, ONNX CPU | 940 | 3 | 9 | 99.68% | 99.05% |
| Linux detector, ONNX CPU | 940 | 3 | 9 | 99.68% | 99.05% |

Deployment tests use confidence 0.50 and matching IoU 0.50. The Mac app and
harness combine Vision object confidence with conditional label confidence.
The earlier Mac report applied the threshold to label confidence alone; for
this single-class model that value is always one. The corrected report is
`validation/adserp-v1/macos-joint-confidence.json`; the earlier report remains
historical evidence of the previous behavior, not a fixed model-confidence
operating point. The installed CoreML package was then re-exported with NMS IoU
0.45 to match the ports, reducing false positives from six to three without
changing recall. Its report is `validation/adserp-v1-nms45/macos-evaluation.json`.
The model weights remain the same search-ad checkpoint. The Rust harness
imports each port's actual preprocessing and decoder, but runs inference on the
Mac's ONNX Runtime 1.24.4 CPU runtime, matching the bundled ports. It does not establish DirectML performance, native Windows
capture behavior or Linux compositor placement. The training validator's
optimized-score metrics were precision 99.50%, recall 98.42%, mAP50 99.45% and
mAP50–95 97.69%; those differ from the fixed deployment operating point.

Reports: [validation/adserp-v1](validation/adserp-v1).

## Sports candidate

A separate training candidate combines the prepared search dataset with public
[ExposureEngine](https://huggingface.co/datasets/SimulaMet-HOST/ExposureEngine)
(MIT as declared in its dataset card), pinned to revision
`a652cd0df28c889535adb0dfdd4aad30652cca6b`. Cite Mehdi Houshmand Sarkhoosh et al.,
*ExposureEngine: Oriented Logo Detection and Sponsor Visibility Analytics in
Sports Broadcasts* (2025), https://arxiv.org/abs/2510.04739.

Original sponsor polygons become enclosing horizontal boxes. Source annotations
are paired globally by frame stem because source folders do not always align.
Unannotated frames and frames with invalid geometry are excluded, with reasons
preserved. Frames from the same clip share one split. The prepared sports set
contains 550 training, 237 validation and 95 test frames; it contains no explicit
negative frames. The combined set retains the search dataset's negatives.

The first combined candidate completed ten epochs at image size 640. Its
independent combined test results were precision 79.95%, recall 58.56%,
mAP50 68.64% and mAP50–95 51.21%. It failed the installation thresholds
(90% precision and 80% recall), so the stronger search-ad starter remains
installed. The second candidate completed twenty epochs at image size 960.
Its independent combined test results were precision 79.39%, recall 69.51%,
mAP50 76.59% and mAP50–95 56.93%; it also failed the installation thresholds.

The exported second candidate was additionally evaluated at the app's fixed
confidence of 0.50 and matching IoU of 0.50, with the test domains scored
separately:

| Candidate deployment path | Test domain | Precision | Recall |
|---|---|---:|---:|
| Mac Vision/CoreML, matched NMS | Search ads (573 images) | 98.95% | 99.16% |
| Mac Vision/CoreML, matched NMS | Sports sponsors (95 images) | 75.92% | 48.43% |
| Linux detector, ONNX CPU | Search ads (573 images) | 99.05% | 99.26% |
| Linux detector, ONNX CPU | Sports sponsors (95 images) | 75.35% | 48.33% |

The Linux detector harness runs on macOS with ONNX Runtime 1.24.4 CPU, as above;
this is not a native Linux inference or compositor validation. CoreML and ONNX
use their respective deployment suppression paths, so their counts differ.
The corrected CoreML export explicitly sets NMS IoU to 0.45, matching the ports;
Ultralytics' export default is 0.70. Its report applies the corrected joint
Vision confidence. The earlier candidate CoreML reports used conditional
label confidence alone and are superseded by
[validation/combined-v2-nms45](validation/combined-v2-nms45).
Reports: [validation/combined-v2](validation/combined-v2). This candidate remains
separate and uninstalled. Small, crowded sponsor logos remain a major source
of missed and incorrect covers; search-ad performance cannot stand in for
sports validation.

A third, separate candidate is training from the official YOLO11 Small
pretrained initialization at image size 960, batch size two, for up to thirty
epochs. It preserves the prepared grouped splits and search negatives. Larger
capacity is being evaluated for crowded sponsor frames; it is not a released
model or evidence of improved accuracy until holdout deployment checks finish.

## Behavior and limits

Automatic detection starts disabled. Only explicit `ad` / `sponsor_remove`
labels authorize removal. Known protect labels suppress overlapping removal
boxes. ONNX metadata is checksum-bound to weights; missing or stale metadata
cannot turn generic objects into removal targets. The supplied public datasets
have no explicit `team_keep` or `number_keep` annotations, so protection remains
a labeling capability rather than a validated model skill.

Accuracy on held-out screenshots does not establish perfect blocking, accuracy
on unfamiliar services, or reliable tracking of every moving broadcast logo.
Horizontal rectangles can cover extra surrounding content. Manual regions remain
available. DRM-protected pixels can remain unavailable or black; the app does not
restore them, bypass DRM, or promise undetectable capture.

Ultralytics training code is [AGPL-3.0](https://github.com/ultralytics/ultralytics/blob/main/LICENSE).
Preserve dataset attribution separately from model and dependency licenses.
