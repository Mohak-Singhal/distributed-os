import AppKit
import CoreImage

/// Generates QR code images using Core Image's CIQRCodeGenerator.
enum QRCodeGenerator {
    /// Generates a QR code NSImage from a plain string with high error correction.
    /// - Parameter string: The payload to encode (e.g. a URL).
    /// - Returns: An NSImage of the QR code, or nil if generation fails.
    static func generate(from string: String) -> NSImage? {
        let data = string.data(using: .utf8)
        guard let filter = CIFilter(name: "CIQRCodeGenerator") else { return nil }
        filter.setValue(data, forKey: "inputMessage")
        filter.setValue("H", forKey: "inputCorrectionLevel")

        guard let ciImage = filter.outputImage else { return nil }

        let transform = CGAffineTransform(scaleX: 10, y: 10)
        let scaled = ciImage.transformed(by: transform)

        let rep = NSCIImageRep(ciImage: scaled)
        let image = NSImage(size: rep.size)
        image.addRepresentation(rep)
        return image
    }
}
