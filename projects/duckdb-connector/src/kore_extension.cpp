#include "duckdb.hpp"
#include "duckdb/parser/parsed_data/create_pragma_function_info.hpp"
#include "duckdb/parser/expression/constant_expression.hpp"
#include "kore_reader.hpp"

using namespace duckdb;

/**
 * DuckDB Extension for Kore File Format
 *
 * Provides read-only access to Kore compressed files from DuckDB.
 *
 * Usage:
 *   LOAD 'kore_extension.so';
 *   SELECT * FROM read_kore('path/to/file.kore');
 *   SELECT * FROM kore_scan('path/to/file.kore') WHERE id > 100;
 */

// Forward declarations
namespace kore {

/**
 * Main extension class for DuckDB.
 */
class KoreExtension : public Extension {
public:
    virtual std::string GetVersion() const {
        return "1.0.0";
    }
    
    virtual std::string GetName() const {
        return "Kore";
    }
    
    virtual void Load(DuckDB &database);
};

// Singleton instance
static KoreExtension *kore_extension = nullptr;

/**
 * Table function to read Kore files.
 * Usage: SELECT * FROM read_kore('file.kore');
 */
struct KoreReadFunction : public TableFunction {
    KoreReadFunction();
};

/**
 * Statistics function for Kore files.
 * Returns file metadata: row count, column count, codecs used.
 */
struct KoreStatsFunction : public ScalarFunction {
    KoreStatsFunction();
};

/**
 * Loads the Kore extension into DuckDB.
 */
void KoreExtension::Load(DuckDB &database) {
    Connection con(database);
    
    // Register table function
    con.BeginTransaction();
    con.RegisterTableFunction(KoreReadFunction());
    con.RegisterScalarFunction(KoreStatsFunction());
    con.Commit();
}

// Pragma to check Kore support
struct KoreInfoFunction : public PragmaFunction {
    KoreInfoFunction() {
        name = "kore_info";
    }
};

} // namespace kore

/**
 * Extension initialization function.
 * Called by DuckDB when extension is loaded.
 */
extern "C" {
    void kore_init(duckdb::DatabaseInstance &instance) {
        // Create extension instance
        if (!kore::kore_extension) {
            kore::kore_extension = new kore::KoreExtension();
        }
        
        // Load into the database instance
        DuckDB db(instance);
        kore::kore_extension->Load(db);
    }
}

/**
 * Implementation of read_kore table function.
 *
 * @param context Execution context
 * @param input Function input parameters
 * @param output Output data chunk
 */
static void KoreReadFunc(
    ClientContext &context,
    TableFunctionInput &data,
    DataChunk &output
) {
    auto &bind_data = (KoreBindData &)*data.bind_data;
    auto &local_state = (KoreLocalState &)*data.local_state;
    auto &global_state = (KoreGlobalState &)*data.global_state;
    
    // Read next chunk from Kore file
    local_state.reader->ReadChunk(output, bind_data.chunk_size);
}

/**
 * Bind function for read_kore.
 * Validates parameters and sets up reading.
 */
static unique_ptr<FunctionData> KoreBind(
    ClientContext &context,
    TableFunctionBindInput &input,
    vector<LogicalType> &return_types,
    vector<string> &names
) {
    if (input.inputs.empty()) {
        throw Exception("read_kore: No filename specified");
    }
    
    // Get filename
    auto filename = input.inputs[0].GetValue<string>();
    
    // Create Kore reader
    auto reader = make_unique<kore::KoreReader>(filename);
    
    // Extract schema
    reader->ReadHeader();
    auto schema = reader->GetSchema();
    
    return_types = schema.types;
    names = schema.names;
    
    auto result = make_unique<KoreBindData>();
    result->filename = filename;
    result->reader = move(reader);
    result->chunk_size = 1024; // Default chunk size
    
    return move(result);
}

/**
 * Initializes the Kore extension.
 * Called when extension is first loaded.
 */
void InitKoreExtension(DatabaseInstance &instance) {
    auto &db = *instance.GetDatabase();
    
    // Register read_kore function
    CreateTableFunctionInfo info;
    info.name = "read_kore";
    info.function = KoreReadFunc;
    info.bind = KoreBind;
    
    db.GetCatalog().CreateTableFunction(move(info));
}
