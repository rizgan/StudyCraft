import sys
import io
import re
import tempfile
import os
import pymupdf
from pymupdf4llm import to_markdown

# --- OCR support (optional — used only when a page has no extractable text) ---
OCR_LANGS = ["eng", "rus", "ara"]
OCR_RENDER_DPI = 300
MIN_TEXT_CHARS_PER_PAGE = 20


def _get_ocr_engine():
    """
    Return a callable ``ocr(image_path) -> str`` or None if no engine is available.

    Preference order:
      1. Tesseract via pytesseract (needs the tesseract binary on PATH, with
         eng/rus/ara language packs installed).
      2. RapidOCR (pure-Python ONNX port of PaddleOCR) — supports en/ru/ar
         without any external binary.
    """
    try:
        import pytesseract
        from PIL import Image

        # Verify the tesseract binary is reachable.
        pytesseract.get_tesseract_version()

        def _tesseract(image_path):
            return pytesseract.image_to_string(
                Image.open(image_path), lang=OCR_LANGS
            )

        return _tesseract
    except Exception:
        pass

    try:
        from rapidocr_onnxruntime import RapidOCR

        engine = RapidOCR()

        def _rapid(image_path):
            result, _elapse = engine(image_path)
            if not result:
                return ""
            return "\n".join(line[1] for line in result)

        return _rapid
    except Exception:
        pass

    return None


def ocr_page(page, dpi=OCR_RENDER_DPI):
    """Render a PyMuPDF page to PNG and run OCR. Returns (text, engine_name)."""
    engine = _get_ocr_engine()
    if engine is None:
        return (
            None,
            "No OCR engine available. Install 'tesseract-ocr' (with eng/rus/ara "
            "language packs) or 'rapidocr_onnxruntime' + 'pillow' in the sidecar "
            "Python environment to enable OCR for scanned PDFs.",
        )

    zoom = dpi / 72.0
    pix = page.get_pixmap(matrix=pymupdf.Matrix(zoom, zoom), alpha=False)
    with tempfile.NamedTemporaryFile(suffix=".png", delete=False) as tmp:
        tmp_path = tmp.name
    try:
        pix.save(tmp_path)
        text = engine(tmp_path) or ""
    finally:
        try:
            os.unlink(tmp_path)
        except OSError:
            pass

    engine_name = "tesseract" if "tesseract" in engine.__module__ else "rapidocr"
    return (text or ""), engine_name


def detect_scan_pages(doc, min_chars=MIN_TEXT_CHARS_PER_PAGE):
    """Return the list of page indices whose extractable text is below threshold."""
    scanned = []
    for i, page in enumerate(doc):
        text = page.get_text("text")
        if len(re.sub(r"\s+", "", text)) < min_chars:
            scanned.append(i)
    return scanned

def clean_tables(text):
    """Remove malformed tables with mostly empty cells"""
    # Remove malformed tables with mostly empty cells
    text = re.sub(r'\|[\|\s-]*\|[\n\r]*', '', text)
    # Remove standalone table separators
    text = re.sub(r'^[\|\-\s]+$', '', text, flags=re.MULTILINE)
    return text

def normalize_whitespace(text):
    """Normalize excessive whitespace from PDF conversion"""
    # Remove extra blank lines (keep max 2 consecutive)
    text = re.sub(r'\n\s*\n\s*\n+', '\n\n', text)
    # Remove trailing spaces
    text = re.sub(r' +$', '', text, flags=re.MULTILINE)
    # Normalize multiple spaces to single space
    text = re.sub(r' {2,}', ' ', text)
    return text

def clean_code_blocks(text):
    """Remove empty or meaningless code blocks"""
    # Remove empty code blocks
    text = re.sub(r'```\s*\n\s*```', '', text)
    # Remove code blocks with just email/simple content
    text = re.sub(r'```\s*\n\s*[\w@.-]+\s*\n\s*```', lambda m: m.group(0).strip('`\n '), text)
    return text

def consolidate_headers(text):
    """Reduce redundant formatting"""
    # Convert excessive header levels to simpler format
    text = re.sub(r'^#{4,} ', '### ', text, flags=re.MULTILINE)
    return text

def optimize_markdown(text):
    """Apply all optimization techniques to reduce token count"""
    text = clean_tables(text)
    text = clean_code_blocks(text)
    text = normalize_whitespace(text)
    text = consolidate_headers(text)
    
    # Remove excessive newlines at start/end
    text = text.strip()
    
    return text

def convert_to_markdown(file_path, optimize=True, use_ocr=False, **kwargs):
    """
    Convert a PDF file to markdown format using pymupdf4llm.

    When ``use_ocr`` is set (or when the document appears to be scanned and no
    OCR engine is missing), pages lacking a text layer are rendered and run
    through the OCR pipeline so that scanned documents still yield usable text
    for the RAG pipeline. Supported OCR languages: English, Russian, Arabic.

    Args:
        file_path: Path to the PDF file
        optimize: Whether to apply token optimization (default: True)
        use_ocr: Force OCR fallback for pages without a text layer.
        **kwargs: Additional arguments passed to pymupdf4llm.to_markdown
    """
    try:
        # Try to open the PDF file
        doc = pymupdf.open(file_path)

        # Detect pages that are scanned (no extractable text layer).
        scanned_pages = detect_scan_pages(doc)
        if scanned_pages and not use_ocr:
            print(
                f"Warning: {len(scanned_pages)}/{len(doc)} pages have no text layer. "
                "Pass --ocr to enable OCR fallback.",
                file=sys.stderr,
            )

        # Convert to markdown using pymupdf4llm
        markdown_text = to_markdown(doc, show_progress=False, **kwargs)

        # OCR fallback: append OCR'd text for scanned pages so the RAG pipeline
        # can index them. OCR output is inserted page-by-page after a marker.
        if scanned_pages and use_ocr:
            ocr_parts = []
            ocr_ok = 0
            for i in scanned_pages:
                page = doc[i]
                text, engine = ocr_page(page)
                if text is None:
                    # Engine unavailable — surface a clear, actionable error.
                    return False, f"Error: {engine}"
                if text.strip():
                    ocr_parts.append(f"\n\n<!-- OCR page {i + 1} ({engine}) -->\n{text.strip()}")
                    ocr_ok += 1
                else:
                    ocr_parts.append(
                        f"\n\n<!-- OCR page {i + 1}: no text detected -->"
                    )
            print(
                f"OCR: processed {ocr_ok}/{len(scanned_pages)} scanned pages.",
                file=sys.stderr,
            )
            markdown_text += "\n\n---\n\n## OCR (scanned pages)\n" + "".join(ocr_parts)

        # Close the document
        doc.close()

        # Apply optimization if requested
        if optimize:
            markdown_text = optimize_markdown(markdown_text)

        return True, markdown_text

    except FileNotFoundError:
        return False, f"Error: File '{file_path}' not found"
    except pymupdf.errors.FileDataError:
        return False, f"Error: '{file_path}' is not a valid PDF file"
    except Exception as e:
        return False, f"Error: {str(e)}"

if __name__ == "__main__":
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')
   
    if len(sys.argv) < 2:
        print("Error: No file path provided", file=sys.stderr)
        sys.exit(1)
       
    file_path = sys.argv[1]

    # Check for optimization and OCR flags (order-independent).
    args = [a for a in sys.argv[2:]]
    optimize = "--no-optimize" not in args
    use_ocr = "--ocr" in args

    # --check-scan: report whether the PDF has pages without a text layer.
    # Used by the Rust side to decide whether to enable OCR automatically.
    if "--check-scan" in args:
        try:
            doc = pymupdf.open(file_path)
            scanned = detect_scan_pages(doc)
            doc.close()
            print("true" if scanned else "false")
            sys.exit(0)
        except Exception as e:
            print(f"Error: {str(e)}", file=sys.stderr)
            sys.exit(1)

    success, result = convert_to_markdown(file_path, optimize=optimize, use_ocr=use_ocr)
   
    if success:
        print(result)
        sys.exit(0)
    else:
        print(result, file=sys.stderr)
        sys.exit(1)