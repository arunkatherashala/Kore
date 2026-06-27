package com.kore.hadoop;

import org.apache.hadoop.fs.Path;
import org.apache.hadoop.io.LongWritable;
import org.apache.hadoop.io.Text;
import org.apache.hadoop.mapreduce.InputFormat;
import org.apache.hadoop.mapreduce.InputSplit;
import org.apache.hadoop.mapreduce.JobContext;
import org.apache.hadoop.mapreduce.RecordReader;
import org.apache.hadoop.mapreduce.TaskAttemptContext;
import org.apache.hadoop.mapreduce.lib.input.FileInputFormat;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.IOException;
import java.util.List;

/**
 * Hadoop InputFormat for Kore compressed file format.
 * Provides integration between Kore files and Hadoop MapReduce.
 *
 * Usage:
 *   Job job = Job.getInstance(config);
 *   job.setInputFormatClass(KoreInputFormat.class);
 *   FileInputFormat.addInputPath(job, new Path("/path/to/kore/files"));
 */
public class KoreInputFormat extends FileInputFormat<LongWritable, KoreRecord> {

    private static final Logger LOG = LoggerFactory.getLogger(KoreInputFormat.class);

    /**
     * File extensions this InputFormat handles.
     */
    private static final String KORE_EXTENSION = ".kore";

    /**
     * Creates a RecordReader for the given split.
     *
     * @param split The InputSplit to read
     * @param context The task attempt context
     * @return A new KoreRecordReader instance
     * @throws IOException If file cannot be opened
     * @throws InterruptedException If interrupted
     */
    @Override
    public RecordReader<LongWritable, KoreRecord> createRecordReader(
            InputSplit split, TaskAttemptContext context)
            throws IOException, InterruptedException {
        
        LOG.info("Creating RecordReader for Kore split: {}", split);
        
        KoreRecordReader reader = new KoreRecordReader();
        reader.initialize(split, context);
        
        return reader;
    }

    /**
     * Validates input paths and sets split properties.
     *
     * @param context The job context
     * @return List of InputSplits
     * @throws IOException If input paths are invalid
     */
    @Override
    public List<InputSplit> getSplits(JobContext context) throws IOException {
        List<InputSplit> splits = super.getSplits(context);
        
        LOG.info("Generated {} splits for Kore input", splits.size());
        
        for (InputSplit split : splits) {
            if (split instanceof KoreFileSplit) {
                KoreFileSplit koreSplit = (KoreFileSplit) split;
                LOG.debug("Split: file={}, offset={}, length={}", 
                    koreSplit.getPath(), 
                    koreSplit.getStart(), 
                    koreSplit.getLength());
            }
        }
        
        return splits;
    }

    /**
     * Checks if a file should be included based on extension and name.
     *
     * @param p The file path to check
     * @return True if file is a Kore file (.kore extension)
     */
    @Override
    protected boolean isSplitable(JobContext context, Path p) {
        String filename = p.getName().toLowerCase();
        
        // Kore files are splittable if they contain block boundaries
        if (filename.endsWith(KORE_EXTENSION)) {
            LOG.debug("File {} is recognized as Kore format", p);
            return true;
        }
        
        return false;
    }

    /**
     * Returns the size of the split in bytes.
     *
     * @param split The InputSplit
     * @return Number of bytes in split
     */
    @Override
    protected long getFormatMinSplitSize() {
        // Minimum split size: 64MB (suitable for compressed data)
        return 64 * 1024 * 1024;
    }
}
