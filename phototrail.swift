import Foundation
import Photos
import CoreLocation
import ImageIO
import SQLite3

// MARK: - Terminal / output helpers

let isTTY = isatty(fileno(stdout)) != 0

func stderrLine(_ s: String) {
    FileHandle.standardError.write((s + "\n").data(using: .utf8)!)
}

func progress(_ s: String) {
    guard isTTY else { return }
    FileHandle.standardError.write("\r\u{1B}[2K\(s)".data(using: .utf8)!)
}

func endProgress() {
    guard isTTY else { return }
    FileHandle.standardError.write("\r\u{1B}[2K".data(using: .utf8)!)
}

func pad(_ s: String, _ width: Int) -> String {
    s.count >= width ? s : s + String(repeating: " ", count: width - s.count)
}

func padLeft(_ s: String, _ width: Int) -> String {
    s.count >= width ? s : String(repeating: " ", count: width - s.count) + s
}

// MARK: - Date formatting

let displayFormatter: DateFormatter = {
    let f = DateFormatter()
    f.dateFormat = "yyyy-MM-dd HH:mm"
    f.locale = Locale(identifier: "en_US_POSIX")
    return f
}()

let displayFormatterSeconds: DateFormatter = {
    let f = DateFormatter()
    f.dateFormat = "yyyy-MM-dd HH:mm:ss"
    f.locale = Locale(identifier: "en_US_POSIX")
    return f
}()

func fmtDate(_ t: Double?) -> String {
    guard let t = t else { return "-" }
    return displayFormatter.string(from: Date(timeIntervalSince1970: t))
}

func parseDate(_ s: String) -> Date? {
    let trimmed = s.trimmingCharacters(in: .whitespacesAndNewlines)
    let iso = ISO8601DateFormatter()
    if let d = iso.date(from: trimmed) { return d }
    let formats = [
        "yyyy-MM-dd'T'HH:mm:ssZ",
        "yyyy-MM-dd'T'HH:mm:ss",
        "yyyy-MM-dd HH:mm:ss",
        "yyyy-MM-dd HH:mm",
        "yyyy-MM-dd",
        "MM/dd/yyyy HH:mm:ss",
        "MM/dd/yyyy HH:mm",
        "MM/dd/yyyy",
    ]
    let f = DateFormatter()
    f.locale = Locale(identifier: "en_US_POSIX")
    f.timeZone = TimeZone.current
    for fmt in formats {
        f.dateFormat = fmt
        if let d = f.date(from: trimmed) { return d }
    }
    return nil
}

func formatDelta(_ seconds: Double) -> String {
    let total = Int(abs(seconds.rounded()))
    let sign = seconds < 0 ? "-" : "+"
    let days = total / 86400
    let hours = (total % 86400) / 3600
    let mins = (total % 3600) / 60
    let secs = total % 60
    if days > 0 { return "\(sign)\(days)d \(hours)h" }
    if hours > 0 { return "\(sign)\(hours)h \(String(format: "%02d", mins))m" }
    if mins > 0 { return "\(sign)\(mins)m \(String(format: "%02d", secs))s" }
    return "\(sign)\(secs)s"
}

// MARK: - SQLite wrapper

let SQLITE_TRANSIENT = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

final class DB {
    var handle: OpaquePointer?

    init(path: String) {
        if sqlite3_open(path, &handle) != SQLITE_OK {
            let msg = handle != nil ? String(cString: sqlite3_errmsg(handle)) : "unknown"
            stderrLine("Unable to open database at \(path): \(msg)")
            exit(1)
        }
        exec("PRAGMA journal_mode=WAL;")
        exec("PRAGMA synchronous=NORMAL;")
    }

    deinit { sqlite3_close(handle) }

    @discardableResult
    func exec(_ sql: String) -> Bool {
        var err: UnsafeMutablePointer<CChar>?
        if sqlite3_exec(handle, sql, nil, nil, &err) != SQLITE_OK {
            let msg = err != nil ? String(cString: err!) : "unknown"
            stderrLine("SQL error: \(msg)")
            sqlite3_free(err)
            return false
        }
        return true
    }

    func prepare(_ sql: String) -> OpaquePointer? {
        var stmt: OpaquePointer?
        if sqlite3_prepare_v2(handle, sql, -1, &stmt, nil) != SQLITE_OK {
            stderrLine("Prepare failed: \(String(cString: sqlite3_errmsg(handle)))")
            return nil
        }
        return stmt
    }
}

func bindText(_ stmt: OpaquePointer?, _ idx: Int32, _ value: String?) {
    if let v = value {
        sqlite3_bind_text(stmt, idx, v, -1, SQLITE_TRANSIENT)
    } else {
        sqlite3_bind_null(stmt, idx)
    }
}

func bindDouble(_ stmt: OpaquePointer?, _ idx: Int32, _ value: Double?) {
    if let v = value { sqlite3_bind_double(stmt, idx, v) } else { sqlite3_bind_null(stmt, idx) }
}

func columnText(_ stmt: OpaquePointer?, _ idx: Int32) -> String? {
    guard let c = sqlite3_column_text(stmt, idx) else { return nil }
    return String(cString: c)
}

func columnDoubleOpt(_ stmt: OpaquePointer?, _ idx: Int32) -> Double? {
    if sqlite3_column_type(stmt, idx) == SQLITE_NULL { return nil }
    return sqlite3_column_double(stmt, idx)
}

func setupSchema(_ db: DB) {
    db.exec("""
    CREATE TABLE IF NOT EXISTS photos (
        id TEXT PRIMARY KEY,
        creation_date REAL,
        latitude REAL,
        longitude REAL,
        camera_id TEXT,
        filename TEXT
    );
    """)
    db.exec("CREATE INDEX IF NOT EXISTS idx_photos_date ON photos(creation_date);")
    db.exec("CREATE INDEX IF NOT EXISTS idx_photos_camera ON photos(camera_id);")
    db.exec("""
    CREATE TABLE IF NOT EXISTS cameras (
        id TEXT PRIMARY KEY,
        make TEXT,
        model TEXT,
        lens TEXT,
        serial TEXT
    );
    """)
    db.exec("CREATE TABLE IF NOT EXISTS selected_cameras (camera_id TEXT PRIMARY KEY);")
}

// MARK: - Camera model

struct CameraRow {
    let id: String
    let make: String
    let model: String
    let serial: String?
    let count: Int
    let withGPS: Int
    let first: Double?
    let last: Double?

    var displayName: String {
        let name = "\(make) \(model)".trimmingCharacters(in: .whitespaces)
        return name.isEmpty ? id : name
    }

    var identifier: String {
        if let s = serial, !s.isEmpty { return s }
        return model.isEmpty ? "(unknown)" : model
    }
}

func cameraKey(make: String?, model: String?, serial: String?) -> String {
    let mk = (make ?? "Unknown").trimmingCharacters(in: .whitespaces)
    let md = (model ?? "Unknown").trimmingCharacters(in: .whitespaces)
    if let s = serial, !s.isEmpty {
        return "\(mk)|\(md)|\(s)"
    }
    return "\(mk)|\(md)"
}

func fetchCamerasOrdered(_ db: DB) -> [CameraRow] {
    let sql = """
    SELECT c.id, c.make, c.model, c.serial,
           COUNT(p.id) AS cnt,
           SUM(CASE WHEN p.latitude IS NOT NULL THEN 1 ELSE 0 END) AS withgps,
           MIN(p.creation_date), MAX(p.creation_date)
    FROM cameras c
    LEFT JOIN photos p ON p.camera_id = c.id
    GROUP BY c.id
    ORDER BY cnt DESC, c.id ASC;
    """
    guard let stmt = db.prepare(sql) else { return [] }
    defer { sqlite3_finalize(stmt) }
    var rows = [CameraRow]()
    while sqlite3_step(stmt) == SQLITE_ROW {
        rows.append(CameraRow(
            id: columnText(stmt, 0) ?? "",
            make: columnText(stmt, 1) ?? "Unknown",
            model: columnText(stmt, 2) ?? "Unknown",
            serial: columnText(stmt, 3),
            count: Int(sqlite3_column_int(stmt, 4)),
            withGPS: Int(sqlite3_column_int(stmt, 5)),
            first: columnDoubleOpt(stmt, 6),
            last: columnDoubleOpt(stmt, 7)
        ))
    }
    return rows
}

func selectedCameraIDs(_ db: DB) -> [String] {
    guard let stmt = db.prepare("SELECT camera_id FROM selected_cameras;") else { return [] }
    defer { sqlite3_finalize(stmt) }
    var ids = [String]()
    while sqlite3_step(stmt) == SQLITE_ROW {
        if let id = columnText(stmt, 0) { ids.append(id) }
    }
    return ids
}

// MARK: - PhotoKit access

func authorize() -> Bool {
    let sem = DispatchSemaphore(value: 0)
    var granted = false
    PHPhotoLibrary.requestAuthorization(for: .readWrite) { status in
        granted = (status == .authorized || status == .limited)
        sem.signal()
    }
    sem.wait()
    return granted
}

struct CameraInfo {
    var make: String?
    var model: String?
    var lens: String?
    var serial: String?

    var hasCamera: Bool { make != nil || model != nil }
}

func parseCameraInfo(_ props: [CFString: Any]) -> CameraInfo {
    var info = CameraInfo()
    if let tiff = props[kCGImagePropertyTIFFDictionary] as? [CFString: Any] {
        info.make = (tiff[kCGImagePropertyTIFFMake] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
        info.model = (tiff[kCGImagePropertyTIFFModel] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
    }
    if let exif = props[kCGImagePropertyExifDictionary] as? [CFString: Any] {
        info.lens = exif[kCGImagePropertyExifLensModel] as? String
        info.serial = (exif[kCGImagePropertyExifBodySerialNumber] as? String)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }
    return info
}

func imageResource(for asset: PHAsset) -> PHAssetResource? {
    let resources = PHAssetResource.assetResources(for: asset)
    let priority: [PHAssetResourceType] = [.photo, .fullSizePhoto, .alternatePhoto]
    for type in priority {
        if let r = resources.first(where: { $0.type == type }) { return r }
    }
    return resources.first
}

// Reads only the EXIF/TIFF header rather than the whole image: the resource is
// streamed in chunks into an incremental image source, and the request is
// cancelled as soon as the camera metadata is available. For iCloud-only assets
// this avoids downloading the full original.
func extractCameraInfo(for asset: PHAsset, allowNetwork: Bool) -> CameraInfo? {
    guard let resource = imageResource(for: asset) else { return nil }

    let options = PHAssetResourceRequestOptions()
    options.isNetworkAccessAllowed = allowNetwork

    let manager = PHAssetResourceManager.default()
    let imageSource = CGImageSourceCreateIncremental(nil)
    let maxBytes = 4 * 1024 * 1024

    let lock = NSLock()
    var buffer = Data()
    var info: CameraInfo? = nil
    var finished = false
    var requestID: PHAssetResourceDataRequestID = 0
    var pendingCancel = false
    let sem = DispatchSemaphore(value: 0)

    // Must be called with `lock` held.
    func stop(with parsed: CameraInfo?) {
        if finished { return }
        finished = true
        info = parsed
        if requestID != 0 {
            manager.cancelDataRequest(requestID)
        } else {
            pendingCancel = true
        }
        sem.signal()
    }

    let id = manager.requestData(for: resource, options: options, dataReceivedHandler: { chunk in
        lock.lock()
        defer { lock.unlock() }
        if finished { return }
        buffer.append(chunk)
        CGImageSourceUpdateData(imageSource, buffer as CFData, false)
        if let props = CGImageSourceCopyPropertiesAtIndex(imageSource, 0, nil) as? [CFString: Any] {
            let parsed = parseCameraInfo(props)
            if parsed.hasCamera {
                stop(with: parsed)
                return
            }
        }
        if buffer.count >= maxBytes {
            CGImageSourceUpdateData(imageSource, buffer as CFData, true)
            let props = CGImageSourceCopyPropertiesAtIndex(imageSource, 0, nil) as? [CFString: Any]
            stop(with: props.map(parseCameraInfo))
        }
    }, completionHandler: { _ in
        lock.lock()
        defer { lock.unlock() }
        if finished { return }
        finished = true
        CGImageSourceUpdateData(imageSource, buffer as CFData, true)
        if let props = CGImageSourceCopyPropertiesAtIndex(imageSource, 0, nil) as? [CFString: Any] {
            info = parseCameraInfo(props)
        }
        sem.signal()
    })

    lock.lock()
    requestID = id
    let cancelNow = pendingCancel
    lock.unlock()
    if cancelNow { manager.cancelDataRequest(id) }

    sem.wait()
    return info
}

func originalFilename(for asset: PHAsset) -> String? {
    PHAssetResource.assetResources(for: asset).first?.originalFilename
}

// MARK: - Commands

func runAnalyze(db: DB, full: Bool, allowNetwork: Bool) {
    guard authorize() else {
        stderrLine("Photos access was not granted.")
        stderrLine("Grant access in System Settings > Privacy & Security > Photos, then re-run.")
        exit(1)
    }

    if full {
        db.exec("DELETE FROM photos;")
        db.exec("DELETE FROM cameras;")
    }

    var existing = Set<String>()
    if !full {
        if let stmt = db.prepare("SELECT id FROM photos;") {
            while sqlite3_step(stmt) == SQLITE_ROW {
                if let id = columnText(stmt, 0) { existing.insert(id) }
            }
            sqlite3_finalize(stmt)
        }
    }

    let fetchOptions = PHFetchOptions()
    fetchOptions.includeHiddenAssets = false
    let assets = PHAsset.fetchAssets(with: .image, options: fetchOptions)
    let total = assets.count

    print("Found \(total) photos in library.")
    if !existing.isEmpty {
        print("Resuming: \(existing.count) already analyzed (use --full to rescan everything).")
    }
    if !allowNetwork {
        print("Reading only locally-available data (pass --download to fetch iCloud originals).")
    }

    let insertPhoto = db.prepare(
        "INSERT OR REPLACE INTO photos (id, creation_date, latitude, longitude, camera_id, filename) VALUES (?, ?, ?, ?, ?, ?);")
    let insertCamera = db.prepare(
        "INSERT OR REPLACE INTO cameras (id, make, model, lens, serial) VALUES (?, ?, ?, ?, ?);")

    let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
    var processed = 0
    var newlyAdded = 0
    var gpsCount = 0
    var unknownCamera = 0
    var camerasSeen = Set<String>()

    db.exec("BEGIN TRANSACTION;")

    assets.enumerateObjects { asset, _, _ in
        processed += 1

        if existing.contains(asset.localIdentifier) {
            if processed % 25 == 0 || processed == total {
                let frame = spinner[processed % spinner.count]
                let pct = total > 0 ? Double(processed) / Double(total) * 100 : 0
                progress("\(frame) \(processed)/\(total) (\(String(format: "%.1f", pct))%) — skipping known")
            }
            return
        }

        let date = asset.creationDate?.timeIntervalSince1970
        var lat: Double? = nil
        var lon: Double? = nil
        if let loc = asset.location {
            lat = loc.coordinate.latitude
            lon = loc.coordinate.longitude
            gpsCount += 1
        }

        let info = extractCameraInfo(for: asset, allowNetwork: allowNetwork)
        var camID: String
        if let info = info, info.hasCamera {
            camID = cameraKey(make: info.make, model: info.model, serial: info.serial)
            sqlite3_reset(insertCamera)
            bindText(insertCamera, 1, camID)
            bindText(insertCamera, 2, info.make ?? "Unknown")
            bindText(insertCamera, 3, info.model ?? "Unknown")
            bindText(insertCamera, 4, info.lens)
            bindText(insertCamera, 5, info.serial)
            sqlite3_step(insertCamera)
        } else {
            camID = "Unknown|Unknown"
            unknownCamera += 1
            sqlite3_reset(insertCamera)
            bindText(insertCamera, 1, camID)
            bindText(insertCamera, 2, "Unknown")
            bindText(insertCamera, 3, "Unknown")
            bindText(insertCamera, 4, nil)
            bindText(insertCamera, 5, nil)
            sqlite3_step(insertCamera)
        }
        camerasSeen.insert(camID)

        sqlite3_reset(insertPhoto)
        bindText(insertPhoto, 1, asset.localIdentifier)
        bindDouble(insertPhoto, 2, date)
        bindDouble(insertPhoto, 3, lat)
        bindDouble(insertPhoto, 4, lon)
        bindText(insertPhoto, 5, camID)
        bindText(insertPhoto, 6, originalFilename(for: asset))
        sqlite3_step(insertPhoto)

        newlyAdded += 1

        if newlyAdded % 500 == 0 {
            db.exec("COMMIT TRANSACTION;")
            db.exec("BEGIN TRANSACTION;")
        }

        if processed % 5 == 0 || processed == total {
            let frame = spinner[processed % spinner.count]
            let pct = total > 0 ? Double(processed) / Double(total) * 100 : 0
            progress("\(frame) \(processed)/\(total) (\(String(format: "%.1f", pct))%) — "
                + "\(camerasSeen.count) cameras, \(gpsCount) with GPS")
        }
    }

    db.exec("COMMIT TRANSACTION;")
    sqlite3_finalize(insertPhoto)
    sqlite3_finalize(insertCamera)
    endProgress()

    print("Done. Added \(newlyAdded) new photos this run.")
    print("  \(gpsCount) of the new photos have GPS coordinates.")
    if unknownCamera > 0 {
        print("  \(unknownCamera) had no readable camera metadata (grouped under 'Unknown').")
        if !allowNetwork {
            print("  Some of these may be iCloud-only — re-run with --download to read them.")
        }
    }
    print("Run './phototrail list-cameras' to see cameras.")
}

func runListCameras(db: DB) {
    let cameras = fetchCamerasOrdered(db)
    if cameras.isEmpty {
        print("No cameras found. Run './phototrail analyze' first.")
        return
    }
    let selected = Set(selectedCameraIDs(db))

    let header = pad("#", 4) + pad("Make / Model", 32) + pad("Identifier", 22)
        + padLeft("Photos", 9) + padLeft("GPS", 9) + "  "
        + pad("First", 18) + pad("Last", 18) + "Sel"
    print(header)
    print(String(repeating: "-", count: header.count))

    for (i, cam) in cameras.enumerated() {
        let idx = i + 1
        let sel = selected.contains(cam.id) ? " *" : ""
        let line = pad("\(idx)", 4)
            + pad(String(cam.displayName.prefix(31)), 32)
            + pad(String(cam.identifier.prefix(21)), 22)
            + padLeft("\(cam.count)", 9)
            + padLeft("\(cam.withGPS)", 9) + "  "
            + pad(fmtDate(cam.first), 18)
            + pad(fmtDate(cam.last), 18)
            + sel
        print(line)
    }

    print("")
    if selected.isEmpty {
        print("No cameras selected — query uses all cameras.")
        print("Select with: ./phototrail select-cameras <#> [<#> ...]  (or model text)")
    } else {
        print("\(selected.count) camera(s) selected (marked *). Query uses only those.")
    }
}

func runSelectCameras(db: DB, args: [String]) {
    let cameras = fetchCamerasOrdered(db)
    if cameras.isEmpty {
        print("No cameras found. Run './phototrail analyze' first.")
        return
    }
    if args.isEmpty {
        stderrLine("Usage: ./phototrail select-cameras <#> [<#> ...]  (numbers from list-cameras, or model text)")
        exit(1)
    }

    var chosen = [CameraRow]()
    var chosenIDs = Set<String>()

    for arg in args {
        if let idx = Int(arg), idx >= 1, idx <= cameras.count {
            let cam = cameras[idx - 1]
            if chosenIDs.insert(cam.id).inserted { chosen.append(cam) }
            continue
        }
        let needle = arg.lowercased()
        let matches = cameras.filter {
            "\($0.make) \($0.model) \($0.serial ?? "")".lowercased().contains(needle)
        }
        if matches.isEmpty {
            stderrLine("No camera matched '\(arg)'.")
        } else {
            for cam in matches where chosenIDs.insert(cam.id).inserted {
                chosen.append(cam)
            }
        }
    }

    if chosen.isEmpty {
        stderrLine("Nothing selected. Run './phototrail list-cameras' to see valid numbers.")
        exit(1)
    }

    db.exec("DELETE FROM selected_cameras;")
    let stmt = db.prepare("INSERT OR IGNORE INTO selected_cameras (camera_id) VALUES (?);")
    for cam in chosen {
        sqlite3_reset(stmt)
        bindText(stmt, 1, cam.id)
        sqlite3_step(stmt)
    }
    sqlite3_finalize(stmt)

    print("Selected \(chosen.count) camera(s) for the photo trail:")
    for cam in chosen {
        print("  - \(cam.displayName)  (\(cam.withGPS) of \(cam.count) photos have GPS)")
    }
}

func queryPhotos(db: DB, target: Double, cameraIDs: [String], before: Bool, limit: Int) -> [(date: Double, lat: Double, lon: Double, camera: String, filename: String?)] {
    var sql = """
    SELECT creation_date, latitude, longitude, camera_id, filename
    FROM photos
    WHERE latitude IS NOT NULL AND longitude IS NOT NULL
      AND creation_date IS NOT NULL
      AND creation_date \(before ? "<=" : ">") ?
    """
    if !cameraIDs.isEmpty {
        let placeholders = cameraIDs.map { _ in "?" }.joined(separator: ",")
        sql += " AND camera_id IN (\(placeholders))"
    }
    sql += " ORDER BY creation_date \(before ? "DESC" : "ASC") LIMIT \(limit);"

    guard let stmt = db.prepare(sql) else { return [] }
    defer { sqlite3_finalize(stmt) }

    sqlite3_bind_double(stmt, 1, target)
    for (i, id) in cameraIDs.enumerated() {
        bindText(stmt, Int32(2 + i), id)
    }

    var rows = [(date: Double, lat: Double, lon: Double, camera: String, filename: String?)]()
    while sqlite3_step(stmt) == SQLITE_ROW {
        rows.append((
            date: sqlite3_column_double(stmt, 0),
            lat: sqlite3_column_double(stmt, 1),
            lon: sqlite3_column_double(stmt, 2),
            camera: columnText(stmt, 3) ?? "",
            filename: columnText(stmt, 4)
        ))
    }
    return rows
}

func cameraModelLookup(_ db: DB) -> [String: String] {
    var map = [String: String]()
    if let stmt = db.prepare("SELECT id, make, model FROM cameras;") {
        while sqlite3_step(stmt) == SQLITE_ROW {
            let id = columnText(stmt, 0) ?? ""
            let name = "\(columnText(stmt, 1) ?? "") \(columnText(stmt, 2) ?? "")"
                .trimmingCharacters(in: .whitespaces)
            map[id] = name.isEmpty ? id : name
        }
        sqlite3_finalize(stmt)
    }
    return map
}

func runQuery(db: DB, dateString: String) {
    let countStmt = db.prepare("SELECT COUNT(*) FROM photos;")
    var photoCount = 0
    if sqlite3_step(countStmt) == SQLITE_ROW { photoCount = Int(sqlite3_column_int(countStmt, 0)) }
    sqlite3_finalize(countStmt)
    if photoCount == 0 {
        print("No photos in the database. Run './phototrail analyze' first.")
        return
    }

    guard let target = parseDate(dateString) else {
        stderrLine("Could not parse date '\(dateString)'.")
        stderrLine("Try formats like '2024-05-01 14:30', '2024-05-01', or ISO 8601 '2024-05-01T14:30:00Z'.")
        exit(1)
    }

    let t = target.timeIntervalSince1970
    let cameraIDs = selectedCameraIDs(db)
    let models = cameraModelLookup(db)

    print("Target: \(displayFormatterSeconds.string(from: target)) (local time)")
    if cameraIDs.isEmpty {
        print("Cameras: all (no selection — run select-cameras to narrow down)")
    } else {
        let names = cameraIDs.map { models[$0] ?? $0 }.joined(separator: ", ")
        print("Cameras: \(names)")
    }
    print("")

    let before = queryPhotos(db: db, target: t, cameraIDs: cameraIDs, before: true, limit: 5)
    let after = queryPhotos(db: db, target: t, cameraIDs: cameraIDs, before: false, limit: 5)

    if before.isEmpty && after.isEmpty {
        print("No geotagged photos found from the selected cameras.")
        return
    }

    func printRow(_ r: (date: Double, lat: Double, lon: Double, camera: String, filename: String?)) {
        let delta = formatDelta(r.date - t)
        let coords = String(format: "%.6f, %.6f", r.lat, r.lon)
        let model = models[r.camera] ?? r.camera
        let name = r.filename ?? ""
        print("  " + pad(delta, 10)
            + pad(displayFormatterSeconds.string(from: Date(timeIntervalSince1970: r.date)), 22)
            + pad(coords, 26)
            + pad(String(model.prefix(20)), 22)
            + name)
    }

    print("Before (closest first):")
    if before.isEmpty {
        print("  (none)")
    } else {
        for r in before { printRow(r) }
    }

    print("After (closest first):")
    if after.isEmpty {
        print("  (none)")
    } else {
        for r in after { printRow(r) }
    }
}

// MARK: - Usage

func printUsage() {
    print("""
    phototrail — build a timestamp+GPS trail from your Apple Photos library.

    Usage:
      ./phototrail analyze [--full] [--download]
          Scan the photo library into phototrail.db.
          --full      Rescan everything (default resumes, skipping known photos).
          --download  Allow fetching iCloud originals to read camera metadata.

      ./phototrail list-cameras
          List cameras with photo counts, GPS counts and date ranges.

      ./phototrail select-cameras <#> [<#> ...]
          Choose which cameras to use (numbers from list-cameras, or model text).

      ./phototrail query "<date>"
          Show up to 5 geotagged photos before and after <date>, closest first.
          Examples: "2024-05-01 14:30", "2024-05-01", "2024-05-01T14:30:00Z"
    """)
}

// MARK: - Main

let arguments = Array(CommandLine.arguments.dropFirst())
guard let command = arguments.first else {
    printUsage()
    exit(1)
}

let dbPath = FileManager.default.currentDirectoryPath + "/phototrail.db"
let db = DB(path: dbPath)
setupSchema(db)

switch command {
case "analyze":
    let full = arguments.contains("--full")
    let allowNetwork = arguments.contains("--download") || arguments.contains("--allow-network")
    runAnalyze(db: db, full: full, allowNetwork: allowNetwork)

case "list-cameras":
    runListCameras(db: db)

case "select-cameras":
    runSelectCameras(db: db, args: Array(arguments.dropFirst()))

case "query":
    guard arguments.count >= 2 else {
        stderrLine("Usage: ./phototrail query \"<date>\"")
        exit(1)
    }
    runQuery(db: db, dateString: arguments[1])

case "help", "-h", "--help":
    printUsage()

default:
    stderrLine("Unknown command: \(command)")
    printUsage()
    exit(1)
}
