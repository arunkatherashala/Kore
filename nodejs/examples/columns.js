// Example 2: Read Specific Columns and Get Stats
const { Kore } = require('kore-fileformat');

async function columnExample() {
  // Read specific column
  console.log('📊 Reading specific column...');
  const names = await Kore.readColumn('scores.kore', 'name');
  console.log('Names:', names);
  console.log('');

  // Get statistics
  console.log('📈 Getting file statistics...');
  const stats = await Kore.getStats('scores.kore');
  console.log('Statistics:');
  console.log(`  - Rows: ${stats.rowCount}`);
  console.log(`  - Columns: ${stats.columnCount}`);
  console.log(`  - File size: ${stats.fileSize} bytes`);
  console.log(`  - Compression: ${(stats.compressionRatio * 100).toFixed(1)}%`);
  console.log(`  - Column names: ${stats.columns.join(', ')}`);
}

columnExample().catch(console.error);
