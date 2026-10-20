// 本地 OCR（Apple Vision），只为造含真实 bbox 的测试 fixture，不参与产品链路。
// 用法：swift vision_ocr.swift <图片> > out.json
import AppKit
import Foundation
import Vision

let args = CommandLine.arguments
guard args.count > 1 else {
    FileHandle.standardError.write("usage: vision_ocr <image>\n".data(using: .utf8)!)
    exit(2)
}
let path = args[1]
guard
    let img = NSImage(contentsOfFile: path),
    let tiff = img.tiffRepresentation,
    let bmp = NSBitmapImageRep(data: tiff),
    let cg = bmp.cgImage
else {
    FileHandle.standardError.write("decode failed: \(path)\n".data(using: .utf8)!)
    exit(1)
}

let W = cg.width
let H = cg.height

let req = VNRecognizeTextRequest()
req.recognitionLevel = .accurate
req.recognitionLanguages = ["zh-Hans", "en-US"]
req.usesLanguageCorrection = true

let handler = VNImageRequestHandler(cgImage: cg, options: [:])
try handler.perform([req])

var blocks: [[String: Any]] = []
for obs in (req.results ?? []) {
    guard let cand = obs.topCandidates(1).first else { continue }
    let b = obs.boundingBox
    // Vision 原点在左下、归一化；转成左上原点的像素坐标
    let x1 = Double(b.minX) * Double(W)
    let x2 = Double(b.maxX) * Double(W)
    let y1 = (1 - Double(b.maxY)) * Double(H)
    let y2 = (1 - Double(b.minY)) * Double(H)
    blocks.append([
        "text": cand.string,
        "x1": (x1 * 10).rounded() / 10,
        "y1": (y1 * 10).rounded() / 10,
        "x2": (x2 * 10).rounded() / 10,
        "y2": (y2 * 10).rounded() / 10,
    ])
}

let out: [String: Any] = ["w": W, "h": H, "blocks": blocks]
let data = try JSONSerialization.data(withJSONObject: out, options: [.sortedKeys])
print(String(data: data, encoding: .utf8)!)
