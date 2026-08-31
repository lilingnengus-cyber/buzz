#!/usr/bin/env xcrun swift

import AppKit
import ApplicationServices
import Foundation

struct Options {
    var processName: String?
    var inputPath: String?
    var timeoutSeconds = 30.0
    var expectations: [String] = []
}

func usage() -> Never {
    FileHandle.standardError.write(
        Data(
            "Usage: macos-ax-text-check.swift (--process NAME | --input PATH) [--timeout SECONDS] --expect TEXT [...]\n"
                .utf8
        )
    )
    exit(2)
}

func parseOptions() -> Options {
    var options = Options()
    var arguments = Array(CommandLine.arguments.dropFirst())

    while !arguments.isEmpty {
        let argument = arguments.removeFirst()
        guard !arguments.isEmpty else { usage() }
        let value = arguments.removeFirst()

        switch argument {
        case "--process":
            options.processName = value
        case "--input":
            options.inputPath = value
        case "--timeout":
            guard let timeout = Double(value), timeout >= 0 else { usage() }
            options.timeoutSeconds = timeout
        case "--expect":
            options.expectations.append(value)
        default:
            usage()
        }
    }

    guard (options.processName == nil) != (options.inputPath == nil),
          !options.expectations.isEmpty
    else {
        usage()
    }
    return options
}

func normalized(_ text: String) -> String {
    text.split(whereSeparator: { $0.isWhitespace }).joined(separator: " ")
}

func stringAttribute(_ attribute: CFString, from element: AXUIElement) -> String? {
    var rawValue: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, attribute, &rawValue) == .success,
          let value = rawValue
    else {
        return nil
    }
    if let string = value as? String {
        return string
    }
    if let number = value as? NSNumber {
        return number.stringValue
    }
    return nil
}

func childElements(of element: AXUIElement) -> [AXUIElement] {
    var rawValue: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, kAXChildrenAttribute as CFString, &rawValue) == .success,
          let children = rawValue as? [AXUIElement]
    else {
        return []
    }
    return children
}

func accessibilityText(for processName: String) -> String? {
    guard let application = NSWorkspace.shared.runningApplications.first(where: {
        $0.localizedName == processName
    }) else {
        return nil
    }

    let root = AXUIElementCreateApplication(application.processIdentifier)
    let textAttributes = [
        kAXTitleAttribute,
        kAXValueAttribute,
        kAXDescriptionAttribute,
        kAXHelpAttribute,
        kAXRoleDescriptionAttribute,
    ]
    var pending: [(AXUIElement, Int)] = [(root, 0)]
    var fragments: [String] = []
    var visitedNodes = 0

    while let (element, depth) = pending.popLast() {
        if visitedNodes >= 20_000 {
            break
        }
        if depth > 40 {
            continue
        }
        visitedNodes += 1
        for attribute in textAttributes {
            if let value = stringAttribute(attribute as CFString, from: element), !value.isEmpty {
                fragments.append(value)
            }
        }
        for child in childElements(of: element).reversed() {
            pending.append((child, depth + 1))
        }
    }

    return normalized(fragments.joined(separator: "\n"))
}

func missingExpectations(in text: String, expectations: [String]) -> [String] {
    let haystack = normalized(text)
    return expectations.filter { !haystack.contains(normalized($0)) }
}

let options = parseOptions()

if let inputPath = options.inputPath {
    do {
        let text = try String(contentsOfFile: inputPath, encoding: .utf8)
        let missing = missingExpectations(in: text, expectations: options.expectations)
        guard missing.isEmpty else {
            FileHandle.standardError.write(Data("Missing accessibility text: \(missing.joined(separator: ", "))\n".utf8))
            exit(1)
        }
        print("Accessibility text verification passed: \(options.expectations.joined(separator: ", "))")
        exit(0)
    } catch {
        FileHandle.standardError.write(Data("Could not read accessibility fixture: \(error)\n".utf8))
        exit(1)
    }
}

guard AXIsProcessTrusted() else {
    FileHandle.standardError.write(
        Data("Accessibility permission is required for this terminal before UI acceptance can run.\n".utf8)
    )
    exit(1)
}

let processName = options.processName ?? ""
let deadline = Date().addingTimeInterval(options.timeoutSeconds)
var lastMissing = options.expectations

repeat {
    if let text = accessibilityText(for: processName) {
        lastMissing = missingExpectations(in: text, expectations: options.expectations)
        if lastMissing.isEmpty {
            print("Accessibility text verification passed for \(processName): \(options.expectations.joined(separator: ", "))")
            exit(0)
        }
    }
    if Date() < deadline {
        Thread.sleep(forTimeInterval: 0.5)
    }
} while Date() < deadline

FileHandle.standardError.write(
    Data("Timed out waiting for \(processName) accessibility text: \(lastMissing.joined(separator: ", "))\n".utf8)
)
exit(1)
