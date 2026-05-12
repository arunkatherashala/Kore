// Example 3: Using the Kore Class for Multiple Operations
const { Kore } = require('kore-fileformat');

async function classExample() {
  const kore = new Kore();

  // Load file
  console.log('📂 Loading KORE file...');
  await kore.load('scores.kore');
  console.log('✅ Loaded\n');

  // Get metadata
  console.log('📋 File metadata:');
  const rowCount = await kore.getRowCount();
  const colCount = await kore.getColumnCount();
  const colNames = await kore.getColumnNames();

  console.log(`  - Rows: ${rowCount}`);
  console.log(`  - Columns: ${colCount}`);
  console.log(`  - Column names: ${colNames.join(', ')}\n`);

  // Read all data
  console.log('📖 Reading all data...');
  const data = await kore.readAll();
  console.log(`✅ Read ${data.length} records`);
  console.log(data);
  console.log('');

  // Read single column
  console.log('📊 Reading "score" column...');
  const scores = await kore.readColumn('score');
  console.log('Scores:', scores);
  console.log('');

  // Calculate average score
  const average = scores.reduce((a, b) => a + b, 0) / scores.length;
  console.log(`✨ Average score: ${average.toFixed(2)}`);
}

classExample().catch(console.error);
