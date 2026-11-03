"""固定したPDF文字抽出を子プロセスで有限実行する。OCR・命令実行・意味採否は行わない。"""
from __future__ import annotations

from io import BytesIO
import json
import logging
import os
from pathlib import Path
import subprocess
import sys


PDF抽出版 = "pypdf-6.20.1/plain-v1"
最大原バイト数 = 2_000_000
最大本文文字数 = 150_000
最大頁数 = 60
最大展開バイト数 = 16_000_000


class _解析警告(logging.Handler):
    def __init__(self):
        super().__init__(logging.WARNING)
        self.発生 = False

    def emit(self, record):
        self.発生 = True


def _文字を読む(raw):
    import pypdf
    from pypdf import PdfReader, apply_configuration

    if pypdf.__version__ != "6.20.1":
        raise ValueError("PDF依存版不一致")
    if not 0 < len(raw) <= 最大原バイト数 or not raw.startswith(b"%PDF-"):
        raise ValueError("PDF原本不正")
    警告 = _解析警告()
    logger = logging.getLogger("pypdf")
    logger.addHandler(警告)
    logger.setLevel(logging.WARNING)
    logger.propagate = False
    # Cloud Run/LinuxはOS上限も設定する。Windowsは同じ展開・頁・出力上限と親の期限。
    if sys.platform == "linux":
        import resource
        resource.setrlimit(resource.RLIMIT_AS, (512 * 1024 * 1024,) * 2)
    制限 = {名: 2_000_000 for 名 in (
        "maximum_declared_stream_length", "array_based_stream_maximum_output_length",
        "lzw_maximum_output_length", "run_length_maximum_output_length",
        "zlib_maximum_output_length", "brotli_maximum_output_length",
        "image_maximum_buffer_size")}
    with apply_configuration(**制限, stream_decoding_work_maximum_length=最大展開バイト数,
                             zlib_maximum_recovery_input_length=最大原バイト数,
                             stream_filters_maximum_length=8, page_tree_maximum_entries=1000,
                             page_tree_maximum_depth=20, xform_maximum_invocations_per_extraction=100,
                             jbig2dec_binary=None):
        reader = PdfReader(BytesIO(raw), strict=True, root_object_recovery_limit=1000)
        if reader.is_encrypted or not 1 <= len(reader.pages) <= 最大頁数:
            raise ValueError("PDF暗号又は頁数上限")
        parts, ranges, offset, expanded = [], [], 0, 0
        for number, page in enumerate(reader.pages, 1):
            contents = page.get_contents()
            if contents is None:
                raise ValueError("PDF文字のない頁")
            expanded += len(contents.get_data())
            if expanded > 最大展開バイト数:
                raise ValueError("PDF展開上限")
            text = page.extract_text(extraction_mode="plain")
            if 警告.発生:
                raise ValueError("PDF解析警告による文字省略を採用しない")
            if type(text) is not str or not text.strip():
                raise ValueError("PDF文字のない頁")
            # 頁間の改行だけを挿入し、抽出文字・内部空白・制御文字は改変しない。
            if parts:
                parts.append("\n")
                offset += 1
            end = offset + len(text)
            if end > 最大本文文字数:
                raise ValueError("PDF文字数上限")
            ranges.append([number, offset, end])
            parts.append(text)
            offset = end
        if 警告.発生:
            raise ValueError("PDF解析警告による文字省略を採用しない")
        return {"版": PDF抽出版, "本文": "".join(parts), "頁範囲": ranges}


def PDF文字を抽出(raw, *, 秒上限=8.0):
    """子は原PDFだけを受け取り、時間超過時は終了・回収する。"""
    if type(raw) is not bytes or not 0 < len(raw) <= 最大原バイト数 or not raw.startswith(b"%PDF-"):
        raise ValueError("PDF原本不正")
    if type(秒上限) not in (int, float) or not 0 < 秒上限 <= 60:
        raise ValueError("PDF時間上限不正")
    env = {名: os.environ[名] for 名 in ("SystemRoot", "WINDIR", "TEMP", "TMP") if 名 in os.environ}
    process = subprocess.Popen([sys.executable, "-I", "-X", "utf8", str(Path(__file__).resolve())],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env,
        creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    try:
        output, _ = process.communicate(raw, timeout=秒上限)
    except BaseException:
        process.kill()
        process.communicate()
        raise
    if process.returncode != 0 or len(output) > 1_000_000:
        raise ValueError("PDF抽出不成立")
    try:
        value = json.loads(output.decode("utf-8"))
    except (ValueError, UnicodeError) as exc:
        raise ValueError("PDF抽出交換不正") from exc
    if type(value) is not dict or set(value) != {"版", "本文", "頁範囲"} or value["版"] != PDF抽出版:
        raise ValueError("PDF抽出交換不正")
    text, pages = value["本文"], value["頁範囲"]
    if type(text) is not str or not text.strip() or len(text) > 最大本文文字数:
        raise ValueError("PDF抽出本文不正")
    if type(pages) is not list or not 1 <= len(pages) <= 最大頁数:
        raise ValueError("PDF抽出頁不正")
    offset = 0
    for number, row in enumerate(pages, 1):
        if (type(row) is not list or len(row) != 3 or any(type(x) is not int for x in row)
                or row[0] != number or row[1] != offset or not offset < row[2] <= len(text)):
            raise ValueError("PDF抽出頁対応不正")
        offset = row[2]
        if number < len(pages):
            if text[offset:offset + 1] != "\n":
                raise ValueError("PDF抽出頁区切り不正")
            offset += 1
    if offset != len(text):
        raise ValueError("PDF抽出頁対応不正")
    return text, tuple(tuple(row) for row in pages)


if __name__ == "__main__":
    try:
        value = _文字を読む(sys.stdin.buffer.read(最大原バイト数 + 1))
        sys.stdout.buffer.write(json.dumps(value, ensure_ascii=False, allow_nan=False).encode("utf-8"))
    except Exception:
        # 不信任PDFの例外自由文・メタデータ・パスを返さない。
        raise SystemExit(1)
