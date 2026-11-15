"""Download Bangumi character albums and turn illustrations into transparent PNGs."""

import argparse
import hashlib
import json
import os
import sys
from pathlib import Path
from tempfile import NamedTemporaryFile
from unicodedata import normalize
from urllib.parse import urlsplit

import numpy as np
import onnxruntime as ort
import requests
from PIL import Image, ImageChops, ImageDraw, ImageOps
from requests.adapters import HTTPAdapter
from urllib3.util.retry import Retry


MODEL_URL = (
    "https://github.com/danielgatis/rembg/releases/download/v0.0.0/isnet-anime.onnx"
)
MODEL_SHA256 = "f15622d853e8260172812b657053460e20806f04b9e05147d49af7bed31a6e99"
MEAN = np.array([0.485, 0.456, 0.406], dtype=np.float32)
TIMEOUT = (10, 30)
IMAGE_SUFFIXES = {".jpg", ".jpeg", ".png", ".webp"}


def http_session(proxy):
    session = requests.Session()
    session.headers["User-Agent"] = "GalFriends/1.0 (Bangumi character album tool)"
    if proxy:
        session.trust_env = False
        session.proxies.update(http=proxy, https=proxy)
    retries = Retry(total=2, backoff_factor=0.5, status_forcelist=(429, 502, 503, 504))
    session.mount("https://", HTTPAdapter(max_retries=retries))
    return session


def exact_name(name):
    return "".join(normalize("NFKC", name).casefold().split())


def find_character(session, name):
    matches = {}
    candidates = []
    offset = 0
    while True:
        response = session.post(
            "https://api.bgm.tv/v0/search/characters",
            json={"keyword": name}, params={"limit": 20, "offset": offset}, timeout=TIMEOUT,
        )
        response.raise_for_status()
        page = response.json()
        for entry in page["data"]:
            if len(candidates) < 5:
                candidates.append(f"{entry['name']} ({entry['id']})")
            if exact_name(entry["name"]) == exact_name(name):
                matches[entry["id"]] = entry["name"]
        offset += len(page["data"])
        if not page["data"] or offset >= page["total"] or len(matches) > 1:
            break
    if len(matches) == 1:
        return next(iter(matches))
    options = [f"{label} ({key})" for key, label in matches.items()] or candidates
    raise ValueError(f"No unique exact match for {name!r}. Use --id. Candidates: {', '.join(options)}")


def download_file(session, url, path, sha256=None):
    path.parent.mkdir(parents=True, exist_ok=True)
    with NamedTemporaryFile(dir=path.parent, delete=False) as temp:
        partial = Path(temp.name)
    try:
        digest = hashlib.sha256() if sha256 else None
        with session.get(url, stream=True, timeout=TIMEOUT) as response, partial.open("wb") as output:
            response.raise_for_status()
            for chunk in response.iter_content(256 * 1024):
                output.write(chunk)
                if digest:
                    digest.update(chunk)
        if digest:
            if digest.hexdigest() != sha256:
                raise ValueError(f"SHA-256 mismatch for {url}")
        else:
            with Image.open(partial) as source:
                source.load()
        partial.replace(path)
    finally:
        partial.unlink(missing_ok=True)


def download_album(session, character_id, output):
    url = f"https://next.bgm.tv/p1/characters/{character_id}/photos"
    manifest = {}
    offset = 0
    while True:
        response = session.get(url, params={"limit": 24, "offset": offset}, timeout=TIMEOUT)
        response.raise_for_status()
        page = response.json()
        for photo in page["data"]:
            image_url = photo["images"]["large"]
            suffix = Path(urlsplit(image_url).path).suffix.lower()
            if suffix not in IMAGE_SUFFIXES:
                raise ValueError(f"Unexpected image URL: {image_url}")
            photo_id = int(photo["id"])
            filename = f"{photo_id}{suffix}"
            path = output / "originals" / filename
            if not path.is_file():
                download_file(session, image_url, path)
            manifest[photo_id] = {
                "id": photo_id, "title": photo["title"], "spoiler": photo["spoiler"],
                "file": filename, "image": image_url, "source": photo["comment"],
            }
        offset += len(page["data"])
        if offset >= page["total"]:
            break
        if not page["data"]:
            raise ValueError("Incomplete album: paging stopped before reaching the reported total")
    if len(manifest) != page["total"]:
        raise ValueError("The album changed while paging. Run the download again.")
    output.mkdir(parents=True, exist_ok=True)
    (output / "album.json").write_text(
        json.dumps(list(manifest.values()), ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return len(manifest)


def default_model_path():
    cache_root = Path(os.environ.get("LOCALAPPDATA", Path.home() / ".cache"))
    return cache_root / "GalFriends" / "models" / "isnet-anime.onnx"


class AnimeSegmenter:
    def __init__(self, model_path, threads):
        options = ort.SessionOptions()
        options.intra_op_num_threads = threads
        self.session = ort.InferenceSession(
            str(model_path), sess_options=options, providers=["CPUExecutionProvider"]
        )
        self.input_name = self.session.get_inputs()[0].name

    def mask(self, image):
        sample = image.resize((1024, 1024), Image.Resampling.LANCZOS)
        rgb = np.asarray(sample, dtype=np.float32)
        normalized = rgb / max(float(rgb.max()), 1e-6) - MEAN
        tensor = np.ascontiguousarray(normalized.transpose(2, 0, 1)[None])
        output_name = self.session.get_outputs()[0].name
        (result,) = self.session.run([output_name], {self.input_name: tensor})
        scores = result[0, 0]
        low, high = float(scores.min()), float(scores.max())
        if not np.isfinite([low, high]).all() or high - low <= 1e-6:
            raise ValueError("Segmentation did not find a usable foreground")
        alpha = Image.fromarray(np.rint((scores - low) * (255 / (high - low))).astype(np.uint8))
        return alpha.resize(image.size, Image.Resampling.LANCZOS)

    def cutout(self, image, alpha_curve, keep_polygons):
        rgba = image.convert("RGBA")
        if rgba.getchannel("A").getextrema() != (255, 255):
            return rgba
        alpha = self.mask(rgba.convert("RGB")).point(alpha_curve)
        if keep_polygons:
            scale = 4
            keep = Image.new("L", (rgba.width * scale, rgba.height * scale))
            draw = ImageDraw.Draw(keep)
            for polygon in keep_polygons:
                if len(polygon) < 3 or any(not (0 <= x < rgba.width and 0 <= y < rgba.height) for x, y in polygon):
                    raise ValueError("Keep polygons need at least three points within the source image")
                draw.polygon([(x * scale, y * scale) for x, y in polygon], fill=255)
            alpha = ImageChops.lighter(alpha, keep.resize(rgba.size, Image.Resampling.LANCZOS))
        rgba.putalpha(alpha)
        # Keep foreground RGB unchanged and zero pixels outside the cutout.
        background = alpha.point([255] + [0] * 255)
        rgba.paste((0, 0, 0, 0), mask=background)
        return rgba


def cutout_directory(input_dir, output_dir, segmenter, excluded, alpha_range, keep, manifest=None):
    if input_dir.resolve() == output_dir.resolve():
        raise ValueError("Choose a separate output directory for PNGs.")
    if manifest is None:
        entries = sorted(path for path in input_dir.iterdir() if path.is_file())
    else:
        entries = [input_dir / photo["file"] for photo in manifest]
    paths = [
        path for path in entries
        if path.suffix.lower() in IMAGE_SUFFIXES and path.stem not in excluded
    ]
    if not paths:
        raise ValueError("No images to process")
    if len({path.stem.casefold() for path in paths}) != len(paths):
        raise ValueError("Input files have conflicting output names.")
    low, high = alpha_range
    curve = [round(255 * min(1, max(0, (value - low) / (high - low)))) for value in range(256)]
    output_dir.mkdir(parents=True, exist_ok=True)
    for path in paths:
        with Image.open(path) as source:
            result = segmenter.cutout(ImageOps.exif_transpose(source), curve, keep.get(path.stem, []))
        destination = output_dir / f"{path.stem}.png"
        result.save(destination)
        print(f"{path.name} -> {destination.name}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--proxy", help="HTTP proxy, e.g. http://127.0.0.1:7890")
    commands = parser.add_subparsers(dest="command", required=True)

    fetch = commands.add_parser("fetch", help="Download an album and its metadata")
    character = fetch.add_mutually_exclusive_group(required=True)
    character.add_argument("--name")
    character.add_argument("--id", type=int)
    fetch.add_argument("output", type=Path)

    cutout = commands.add_parser("cutout", help="Create transparent PNGs from local images")
    cutout.add_argument("input", type=Path, help="Image folder or downloaded album folder")
    cutout.add_argument("output", type=Path)
    cutout.add_argument("--exclude", action="append", default=[], help="Skip this image ID or file stem")
    cutout.add_argument("--model", type=Path, help="Use a local ISNet Anime ONNX model")
    cutout.add_argument("--keep", type=Path, help="JSON map from file stems to foreground polygons in source pixels")
    cutout.add_argument("--threads", type=int, default=1)
    cutout.add_argument(
        "--alpha-range", nargs=2, type=int, metavar=("LOW", "HIGH"), default=(38, 225),
        help="Make mask scores at LOW transparent and at HIGH opaque (default: 38 225)",
    )
    args = parser.parse_args()

    with http_session(args.proxy) as session:
        if args.command == "fetch":
            character_id = args.id if args.id is not None else find_character(session, args.name)
            count = download_album(session, character_id, args.output)
            print(f"Character {character_id}: {count} album images")
            return

        low, high = args.alpha_range
        if not 0 <= low < high <= 255:
            cutout.error("--alpha-range must satisfy 0 <= LOW < HIGH <= 255")
        if args.threads < 1:
            cutout.error("--threads must be positive")
        model = args.model or default_model_path()
        if args.model is None and not model.is_file():
            print("Downloading ISNet Anime weights (176 MB)...", flush=True)
            download_file(session, MODEL_URL, model, MODEL_SHA256)
        manifest_path = args.input / "album.json"
        manifest = None
        image_dir = args.input
        if manifest_path.is_file():
            manifest = json.loads(manifest_path.read_text(encoding="utf-8-sig"))
            image_dir = args.input / "originals"
        keep = json.loads(args.keep.read_text(encoding="utf-8-sig")) if args.keep else {}
        cutout_directory(
            image_dir, args.output, AnimeSegmenter(model, args.threads),
            set(args.exclude), args.alpha_range, keep, manifest,
        )


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, requests.RequestException) as error:
        print(f"Error: {error}", file=sys.stderr)
        sys.exit(1)
