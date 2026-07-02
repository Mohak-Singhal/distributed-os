import Foundation
import SharingService

class ShareViewController: SharingService.SharingServiceViewController {

    override var title: String? {
        get { "Send via PDOS" }
        set { }
    }

    override func loadView() {
        let hostingController = NSHostingController(rootView: ShareExtensionView(controller: self))
        self.view = hostingController.view
    }

    override func didSelectItem(at index: Int) {
        completeRequest()
    }

    override func cancel() {
        cancelWithError(NSError(domain: "PDOS", code: 0, userInfo: nil))
    }

    func sendFile(url: URL, deviceID: String) {
        FileTransferService.sendFile(url: url, deviceID: deviceID) { [weak self] _, error in
            DispatchQueue.main.async {
                if error != nil {
                    self?.cancelWithError(NSError(domain: "PDOS", code: 2, userInfo: [NSLocalizedDescriptionKey: error!]))
                } else {
                    self?.completeRequest()
                }
            }
        }
    }

    private func completeRequest() {
        self.extensionContext!.completeRequest(returningItems: []) { _ in }
    }
}
