# AdSERP model source and attribution

Kayhan Latifzadeh, Jacek Gwizdka, Luis A. Leiva. 2025. *A Versatile Dataset of Mouse and Eye Movements on Search Engine Results Pages*. SIGIR 2025. DOI:10.1145/3726302.3730325

Source: https://zenodo.org/records/15236546

Dataset license: Creative Commons Attribution 4.0 International, https://creativecommons.org/licenses/by/4.0/

Changes: one page per query, viewport crops, exact crop deduplication, conversion of public ad boundaries to normalized YOLO labels, query-grouped train/validation/test splits, and YOLOv8n fine-tuning.

The installed model covers search-result ads. The sports candidate remains separate. See MODEL_CARD.md for evaluation and limits.

Ultralytics training code: AGPL-3.0, https://github.com/ultralytics/ultralytics/blob/main/LICENSE. Dataset attribution does not replace model, application, or dependency license requirements.
