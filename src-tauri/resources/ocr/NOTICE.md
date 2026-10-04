PaddleOCR offline model bundle
==============================

The three ONNX files in this directory are PP-OCRv4 detection/recognition and
the PP-OCR angle-classification model. They are from SWHL/RapidOCR at the
immutable Hugging Face revision
`5e7ff7a3692252dd21f42d8c7fd07b9905a1b114` (Apache-2.0):
https://huggingface.co/SWHL/RapidOCR/tree/5e7ff7a3692252dd21f42d8c7fd07b9905a1b114

SHA-256 checksums:

- `ch_PP-OCRv4_det_infer.onnx`: `d2a7720d45a54257208b1e13e36a8479894cb74155a5efe29462512d42f49da9`
- `ch_PP-OCRv4_rec_infer.onnx`: `48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b`
- `ch_ppocr_mobile_v2.0_cls_infer.onnx`: `e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c`

The pinned Rust dependency downloads the matching CPU ONNX Runtime static
library during build and links it into the application. No separate runtime
DLL needs to be installed or bundled.
