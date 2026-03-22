import CoreBluetooth
import Foundation

// Surfterm BLE UUIDs (must match Rust side)
let serviceUUID = CBUUID(string: "5572F001-7846-4D32-A1A4-5F7A4E3B6C10")
let sessionListCharUUID = CBUUID(string: "5572F002-7846-4D32-A1A4-5F7A4E3B6C10")
let commandCharUUID = CBUUID(string: "5572F003-7846-4D32-A1A4-5F7A4E3B6C10")

class BLEPeripheral: NSObject, CBPeripheralManagerDelegate {
    var peripheralManager: CBPeripheralManager!
    var sessionListChar: CBMutableCharacteristic!
    var commandChar: CBMutableCharacteristic!
    var subscribedCentrals: [CBCentral] = []
    var currentSessionData: Data = "[]".data(using: .utf8)!

    override init() {
        super.init()
        peripheralManager = CBPeripheralManager(delegate: self, queue: nil)
    }

    func peripheralManagerDidUpdateState(_ peripheral: CBPeripheralManager) {
        if peripheral.state == .poweredOn {
            setupService()
            log("BLE powered on")
        } else {
            log("BLE state: \(peripheral.state.rawValue)")
        }
    }

    func setupService() {
        sessionListChar = CBMutableCharacteristic(
            type: sessionListCharUUID,
            properties: [.read, .notify],
            value: nil,
            permissions: [.readable]
        )

        commandChar = CBMutableCharacteristic(
            type: commandCharUUID,
            properties: [.write],
            value: nil,
            permissions: [.writeable]
        )

        let service = CBMutableService(type: serviceUUID, primary: true)
        service.characteristics = [sessionListChar, commandChar]
        peripheralManager.add(service)
    }

    func peripheralManager(_ peripheral: CBPeripheralManager, didAdd service: CBService, error: Error?) {
        if let error = error {
            log("Failed to add service: \(error)")
            return
        }
        log("Service added, starting advertising")
        peripheralManager.startAdvertising([
            CBAdvertisementDataServiceUUIDsKey: [serviceUUID],
            CBAdvertisementDataLocalNameKey: "Surfterm"
        ])
    }

    func peripheralManagerDidStartAdvertising(_ peripheral: CBPeripheralManager, error: Error?) {
        if let error = error {
            log("Advertising failed: \(error)")
        } else {
            log("Advertising started as 'Surfterm'")
        }
    }

    func peripheralManager(_ peripheral: CBPeripheralManager, didReceiveRead request: CBATTRequest) {
        if request.characteristic.uuid == sessionListCharUUID {
            let data = currentSessionData
            if request.offset > data.count {
                peripheral.respond(to: request, withResult: .invalidOffset)
                return
            }
            request.value = data.subdata(in: request.offset..<data.count)
            peripheral.respond(to: request, withResult: .success)
        } else {
            peripheral.respond(to: request, withResult: .requestNotSupported)
        }
    }

    func peripheralManager(_ peripheral: CBPeripheralManager, didReceiveWrite requests: [CBATTRequest]) {
        for request in requests {
            if request.characteristic.uuid == commandCharUUID, let value = request.value {
                // Forward command to Rust via stdout
                let json: [String: Any] = [
                    "type": "command",
                    "data": (try? JSONSerialization.jsonObject(with: value)) ?? [:]
                ]
                if let data = try? JSONSerialization.data(withJSONObject: json),
                   let str = String(data: data, encoding: .utf8) {
                    print(str)
                    fflush(stdout)
                }
                peripheral.respond(to: request, withResult: .success)
            } else {
                peripheral.respond(to: request, withResult: .requestNotSupported)
            }
        }
    }

    func peripheralManager(_ peripheral: CBPeripheralManager, central: CBCentral, didSubscribeTo characteristic: CBCharacteristic) {
        if characteristic.uuid == sessionListCharUUID {
            subscribedCentrals.append(central)
            let json = "{\"type\":\"subscribed\"}"
            print(json)
            fflush(stdout)
            log("Client subscribed")

            // Send current data immediately
            peripheralManager.updateValue(currentSessionData, for: sessionListChar, onSubscribedCentrals: [central])
        }
    }

    func peripheralManager(_ peripheral: CBPeripheralManager, central: CBCentral, didUnsubscribeFrom characteristic: CBCharacteristic) {
        subscribedCentrals.removeAll { $0 == central }
        let json = "{\"type\":\"unsubscribed\"}"
        print(json)
        fflush(stdout)
        log("Client unsubscribed")
    }

    func updateSessions(_ data: Data) {
        currentSessionData = data
        if !subscribedCentrals.isEmpty {
            peripheralManager.updateValue(data, for: sessionListChar, onSubscribedCentrals: nil)
        }
    }

    func log(_ message: String) {
        FileHandle.standardError.write("[\(Date())] \(message)\n".data(using: .utf8)!)
    }
}

// Main
let peripheral = BLEPeripheral()

// Read JSON lines from stdin (sent by Surfterm)
DispatchQueue.global(qos: .utility).async {
    while let line = readLine() {
        guard let data = line.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let type = json["type"] as? String else {
            continue
        }

        if type == "update_sessions", let sessionsData = json["data"] {
            if let payload = try? JSONSerialization.data(withJSONObject: sessionsData) {
                DispatchQueue.main.async {
                    peripheral.updateSessions(payload)
                }
            }
        }
    }
}

RunLoop.main.run()
