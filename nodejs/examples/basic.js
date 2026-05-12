// Example 1: Basic Write and Read
const { Kore } = require('kore-fileformat');

async function basicExample() {
  const schema = {
    fields: [
      { name: 'id', type: 'int64' },
      { name: 'name', type: 'string' },
      { name: 'score', type: 'float64' }
    ]
  };

  const data = [
    { id: 1, name: 'Alice', score: 95.5 },
    { id: 2, name: 'Bob', score: 87.3 },
    { id: 3, name: 'Charlie', score: 92.1 }
  ];

  // Write
  console.log('📝 Writing data to KORE...');
  await Kore.write('scores.kore', schema, data);
  console.log('✅ Write complete\n');

  // Read
  console.log('📖 Reading data from KORE...');
  const result = await Kore.read('scores.kore');
  console.log('✅ Read complete');
  console.log(result);
}

basicExample().catch(console.error);
