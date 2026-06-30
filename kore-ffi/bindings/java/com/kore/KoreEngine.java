package com.kore;

import java.io.*;
import java.net.URI;
import java.net.http.*;
import java.net.http.HttpRequest.BodyPublishers;
import java.net.http.HttpResponse.BodyHandlers;
import java.nio.file.*;
import java.util.*;

/**
 * KoreClient -- Java bindings for the KORE engine via REST API (Java 11+).
 *
 * Start the KORE API server first:
 *   cargo run --release -p kore-api
 *
 * Then use this client:
 *   try (var kore = new KoreClient()) {
 *       kore.loadCsv("sales", "/data/sales.csv");
 *       var rows = kore.query("SELECT region, SUM(amount) FROM sales GROUP BY region");
 *       rows.forEach(System.out::println);
 *   }
 *
 * Default server: http://localhost:3000  (override via KORE_API_URL env var)
 *
 * REST API contract:
 *   POST /sql/load_csv   body: {"session":"<id>","table":"<name>","path":"<path>"}
 *   POST /sql/query      body: {"session":"<id>","sql":"<query>"}
 *   GET  /sql/row_count  body: {"session":"<id>","table":"<name>"}
 *   POST /session/new    returns: {"session":"<id>"}
 *   POST /session/free   body: {"session":"<id>"}
 *   POST /ml/fit         body: {"session":"<id>","model_type":<int>,"param1":<int>,
 *                               "param2":<int>,"x":[[...]],"y":[...]}
 *   POST /ml/predict     body: {"session":"<id>","model_id":"<id>","x":[[...]]}
 */
public final class KoreClient implements AutoCloseable {

    // -------------------------------------------------------------------------
    // Configuration
    // -------------------------------------------------------------------------

    private static final String DEFAULT_BASE_URL = "http://localhost:3000";

    private final String   baseUrl;
    private final HttpClient http;
    private final String   sessionId;

    // Model type constants (mirrors kore.h)
    public static final int RF_REGRESSOR     = 0;
    public static final int RF_CLASSIFIER    = 1;
    public static final int GBM_REGRESSOR    = 2;
    public static final int LINEAR_REGRESSOR = 3;
    public static final int LOGISTIC         = 4;
    public static final int KNN_REGRESSOR    = 5;
    public static final int KNN_CLASSIFIER   = 6;
    public static final int SVM              = 7;

    // -------------------------------------------------------------------------
    // Construction / lifecycle
    // -------------------------------------------------------------------------

    /** Connect to the default server URL from KORE_API_URL env var or localhost:3000. */
    public KoreClient() throws IOException, InterruptedException {
        this(resolveBaseUrl());
    }

    /** Connect to a specific server URL. */
    public KoreClient(String baseUrl) throws IOException, InterruptedException {
        this.baseUrl = baseUrl.replaceAll("/+$", "");
        this.http    = HttpClient.newHttpClient();
        this.sessionId = allocSession();
    }

    private static String resolveBaseUrl() {
        String env = System.getenv("KORE_API_URL");
        return (env != null && !env.isBlank()) ? env : DEFAULT_BASE_URL;
    }

    private String allocSession() throws IOException, InterruptedException {
        String body = post("/session/new", "{}");
        return extractString(body, "session");
    }

    @Override
    public void close() {
        try {
            post("/session/free", jsonObject("session", sessionId));
        } catch (Exception ignored) {}
    }

    // -------------------------------------------------------------------------
    // SQL Session API
    // -------------------------------------------------------------------------

    /**
     * Register a CSV file as a named table in this session.
     *
     * @param table logical table name
     * @param path  path visible to the server process
     */
    public void loadCsv(String table, String path) throws IOException, InterruptedException {
        String payload = "{"
            + "\"session\":" + jsonStr(sessionId) + ","
            + "\"table\":"   + jsonStr(table)     + ","
            + "\"path\":"    + jsonStr(path)
            + "}";
        String resp = post("/sql/load_csv", payload);
        checkOk(resp);
    }

    /**
     * Load a list of maps as a named table.
     *
     * The data is serialised to JSON and sent to the server, which materialises
     * it as a table without requiring a file on disk.
     *
     * @param table logical table name
     * @param rows  list of column-name -> value maps
     */
    public void loadTable(String table, List<Map<String, Object>> rows)
            throws IOException, InterruptedException {
        if (rows == null || rows.isEmpty())
            throw new IllegalArgumentException("rows must not be empty");
        String payload = "{"
            + "\"session\":"  + jsonStr(sessionId)  + ","
            + "\"table\":"    + jsonStr(table)       + ","
            + "\"rows\":"     + toJsonArray(rows)
            + "}";
        String resp = post("/sql/load_table", payload);
        checkOk(resp);
    }

    /**
     * Execute a SQL query and return results as a list of maps.
     *
     * @param sql SQL statement to execute
     * @return ordered list of row maps (column name -> value)
     */
    public List<Map<String, Object>> query(String sql)
            throws IOException, InterruptedException {
        String payload = "{"
            + "\"session\":" + jsonStr(sessionId) + ","
            + "\"sql\":"     + jsonStr(sql)
            + "}";
        String resp = post("/sql/query", payload);
        return parseJsonArray(resp);
    }

    /**
     * Return the row count of a named table.
     *
     * @param table logical table name
     * @return number of rows
     */
    public long rowCount(String table) throws IOException, InterruptedException {
        String payload = "{"
            + "\"session\":" + jsonStr(sessionId) + ","
            + "\"table\":"   + jsonStr(table)
            + "}";
        String resp = post("/sql/row_count", payload);
        return parseLong(resp, "count");
    }

    // -------------------------------------------------------------------------
    // ML API
    // -------------------------------------------------------------------------

    /**
     * Train a model and return an opaque model ID.
     *
     * @param modelType one of the model type constants (e.g. LINEAR_REGRESSOR)
     * @param param1    model-specific parameter (e.g. n_trees)
     * @param param2    model-specific parameter (e.g. max_depth)
     * @param x         feature matrix as list-of-rows
     * @param y         label vector
     * @return server-assigned model ID for use in predict()
     */
    public String fit(int modelType, int param1, int param2,
                      List<List<Double>> x, List<Double> y)
            throws IOException, InterruptedException {
        String payload = "{"
            + "\"session\":"    + jsonStr(sessionId)       + ","
            + "\"model_type\":" + modelType                + ","
            + "\"param1\":"     + param1                   + ","
            + "\"param2\":"     + param2                   + ","
            + "\"x\":"          + toJsonMatrix(x)          + ","
            + "\"y\":"          + toJsonDoubleArray(y)
            + "}";
        String resp = post("/ml/fit", payload);
        return extractString(resp, "model_id");
    }

    /**
     * Run inference with a previously trained model.
     *
     * @param modelId model ID returned by fit()
     * @param x       feature matrix as list-of-rows
     * @return predicted values
     */
    public List<Double> predict(String modelId, List<List<Double>> x)
            throws IOException, InterruptedException {
        String payload = "{"
            + "\"session\":"  + jsonStr(sessionId) + ","
            + "\"model_id\":" + jsonStr(modelId)   + ","
            + "\"x\":"        + toJsonMatrix(x)
            + "}";
        String resp = post("/ml/predict", payload);
        return parseDoubleArray(resp);
    }

    // -------------------------------------------------------------------------
    // HTTP helpers (no third-party JSON library required)
    // -------------------------------------------------------------------------

    private String post(String path, String jsonBody)
            throws IOException, InterruptedException {
        HttpRequest req = HttpRequest.newBuilder()
            .uri(URI.create(baseUrl + path))
            .header("Content-Type", "application/json")
            .POST(BodyPublishers.ofString(jsonBody))
            .build();
        HttpResponse<String> resp = http.send(req, BodyHandlers.ofString());
        if (resp.statusCode() >= 400) {
            throw new IOException(
                "KORE API error " + resp.statusCode() + " on " + path
                + ": " + resp.body()
            );
        }
        return resp.body();
    }

    private static void checkOk(String json) throws IOException {
        if (json.contains("\"error\"")) {
            String msg = extractString(json, "error");
            throw new IOException("KORE error: " + msg);
        }
    }

    /** Minimal JSON string extraction without a full parser. */
    private static String extractString(String json, String key) {
        String marker = "\"" + key + "\":\"";
        int start = json.indexOf(marker);
        if (start < 0) return "";
        start += marker.length();
        int end = json.indexOf('"', start);
        return end < 0 ? json.substring(start) : json.substring(start, end);
    }

    private static long parseLong(String json, String key) {
        String marker = "\"" + key + "\":";
        int start = json.indexOf(marker);
        if (start < 0) return -1;
        start += marker.length();
        int end = start;
        while (end < json.length() && (Character.isDigit(json.charAt(end)) || json.charAt(end) == '-'))
            end++;
        try { return Long.parseLong(json.substring(start, end)); }
        catch (NumberFormatException e) { return -1; }
    }

    /** Very small JSON array-of-objects parser: handles string and number values. */
    @SuppressWarnings("unchecked")
    private static List<Map<String, Object>> parseJsonArray(String json) {
        List<Map<String, Object>> result = new ArrayList<>();
        // Minimal tokeniser -- sufficient for flat row objects
        int i = json.indexOf('[');
        if (i < 0) return result;
        while ((i = json.indexOf('{', i)) >= 0) {
            int end = json.indexOf('}', i);
            if (end < 0) break;
            String obj = json.substring(i + 1, end);
            Map<String, Object> row = new LinkedHashMap<>();
            int pos = 0;
            while (pos < obj.length()) {
                int ks = obj.indexOf('"', pos);
                if (ks < 0) break;
                int ke = obj.indexOf('"', ks + 1);
                if (ke < 0) break;
                String k = obj.substring(ks + 1, ke);
                int colon = obj.indexOf(':', ke);
                if (colon < 0) break;
                int vs = colon + 1;
                while (vs < obj.length() && obj.charAt(vs) == ' ') vs++;
                Object val;
                if (obj.charAt(vs) == '"') {
                    int ve = obj.indexOf('"', vs + 1);
                    val = ve < 0 ? "" : obj.substring(vs + 1, ve);
                    pos = ve < 0 ? obj.length() : ve + 1;
                } else {
                    int ve = vs;
                    while (ve < obj.length() && ",}".indexOf(obj.charAt(ve)) < 0) ve++;
                    String raw = obj.substring(vs, ve).trim();
                    try { val = raw.contains(".") ? Double.parseDouble(raw) : Long.parseLong(raw); }
                    catch (NumberFormatException e) { val = raw; }
                    pos = ve + 1;
                }
                row.put(k, val);
            }
            result.add(row);
            i = end + 1;
        }
        return result;
    }

    private static List<Double> parseDoubleArray(String json) {
        List<Double> out = new ArrayList<>();
        int i = json.indexOf('[');
        if (i < 0) return out;
        int end = json.lastIndexOf(']');
        if (end <= i) return out;
        for (String tok : json.substring(i + 1, end).split(",")) {
            try { out.add(Double.parseDouble(tok.trim())); } catch (NumberFormatException ignored) {}
        }
        return out;
    }

    // -------------------------------------------------------------------------
    // Minimal JSON serialisation (no third-party libs)
    // -------------------------------------------------------------------------

    private static String jsonStr(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"")
                       .replace("\n", "\\n").replace("\r", "\\r") + "\"";
    }

    private static String jsonObject(String key, String value) {
        return "{" + jsonStr(key) + ":" + jsonStr(value) + "}";
    }

    private static String toJsonArray(List<Map<String, Object>> rows) {
        StringBuilder sb = new StringBuilder("[");
        for (int i = 0; i < rows.size(); i++) {
            if (i > 0) sb.append(",");
            sb.append("{");
            boolean first = true;
            for (Map.Entry<String, Object> e : rows.get(i).entrySet()) {
                if (!first) sb.append(",");
                first = false;
                sb.append(jsonStr(e.getKey())).append(":");
                Object v = e.getValue();
                if (v instanceof String) sb.append(jsonStr((String) v));
                else sb.append(v);
            }
            sb.append("}");
        }
        return sb.append("]").toString();
    }

    private static String toJsonMatrix(List<List<Double>> m) {
        StringBuilder sb = new StringBuilder("[");
        for (int i = 0; i < m.size(); i++) {
            if (i > 0) sb.append(",");
            sb.append(toJsonDoubleArray(m.get(i)));
        }
        return sb.append("]").toString();
    }

    private static String toJsonDoubleArray(List<Double> arr) {
        StringBuilder sb = new StringBuilder("[");
        for (int i = 0; i < arr.size(); i++) {
            if (i > 0) sb.append(",");
            sb.append(arr.get(i));
        }
        return sb.append("]").toString();
    }

    // -------------------------------------------------------------------------
    // Demo main
    // -------------------------------------------------------------------------

    public static void main(String[] args) throws Exception {
        System.out.println("=== KORE Java REST client demo ===\n");
        System.out.println("Connecting to " + resolveBaseUrl() + " ...");

        try (KoreClient kore = new KoreClient()) {
            // Load inline table
            List<Map<String, Object>> rows = new ArrayList<>();
            for (int i = 1; i <= 5; i++) {
                Map<String, Object> row = new LinkedHashMap<>();
                row.put("id",    i);
                row.put("value", i * 10.0);
                rows.add(row);
            }
            kore.loadTable("nums", rows);
            System.out.println("Loaded 'nums' table (" + kore.rowCount("nums") + " rows)");

            // SQL query
            var result = kore.query("SELECT SUM(value) AS total FROM nums");
            System.out.println("SELECT SUM(value): " + result);

            // ML: linear regression
            List<List<Double>> X = List.of(
                List.of(1.0), List.of(2.0), List.of(3.0), List.of(4.0)
            );
            List<Double> y = List.of(2.0, 4.0, 6.0, 8.0);
            String modelId = kore.fit(LINEAR_REGRESSOR, 0, 0, X, y);
            System.out.println("Trained model ID: " + modelId);

            List<List<Double>> Xtest = List.of(List.of(5.0), List.of(6.0));
            var preds = kore.predict(modelId, Xtest);
            System.out.println("Predictions for x=5,6: " + preds);
        }

        System.out.println("\nDone.");
    }
}