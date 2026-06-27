import express, { Express, Request, Response } from 'express';
import cors from 'cors';
import helmet from 'helmet';
import compression from 'compression';
import dotenv from 'dotenv';
import * as fs from 'fs';
import * as path from 'path';
import { v4 as uuidv4 } from 'uuid';

dotenv.config();

const app: Express = express();
const port = process.env.PORT || 3000;

// In-memory storage (replace with DB later)
interface FileMetadata {
    id: string;
    name: string;
    size: number;
    createdAt: string;
    updatedAt: string;
    codec: string;
    compressionRatio: number;
}

interface QueryResult {
    id: string;
    fileId: string;
    query: string;
    status: 'pending' | 'running' | 'completed' | 'failed';
    resultSize: number;
    executionTime: number;
    createdAt: string;
    completedAt?: string;
}

const files = new Map<string, FileMetadata>();
const queries = new Map<string, QueryResult>();
const fileStorage = path.join(process.cwd(), '.kore-tmp');

// Ensure storage directory exists
if (!fs.existsSync(fileStorage)) {
    fs.mkdirSync(fileStorage, { recursive: true });
}

// Middleware
app.use(helmet());
app.use(cors());
app.use(compression());
app.use(express.json({ limit: '50mb' }));

// ========== HEALTH ENDPOINTS ==========

// GET /health - Basic health check
app.get('/health', (_req: Request, res: Response) => {
    return res.json({
        status: 'healthy',
        timestamp: new Date().toISOString(),
        uptime: process.uptime(),
        files: files.size,
        queries: queries.size
    });
});

// GET /status - Service status with metrics
app.get('/status', (_req: Request, res: Response) => {
    const fileList = Array.from(files.values());
    const totalSize = fileList.reduce((sum, f) => sum + f.size, 0);
    
    return res.json({
        status: 'operational',
        service: 'kore-cloud-mvp',
        version: '1.0.0',
        timestamp: new Date().toISOString(),
        metrics: {
            files: files.size,
            queries: queries.size,
            totalDataSize: totalSize,
            uptime: Math.round(process.uptime())
        }
    });
});

// ========== FILE ENDPOINTS ==========

// GET /api/v1/files - List all files with metadata
app.get('/api/v1/files', (_req: Request, res: Response) => {
    try {
        const fileList = Array.from(files.values());
        const totalSize = fileList.reduce((sum, f) => sum + f.size, 0);
        const avgRatio = fileList.length > 0 
            ? fileList.reduce((sum, f) => sum + f.compressionRatio, 0) / fileList.length
            : 0;
        
        return res.json({
            status: 'success',
            files: fileList,
            count: fileList.length,
            totalSize: totalSize,
            averageCompressionRatio: avgRatio.toFixed(4)
        });
    } catch (error) {
        return res.status(500).json({ error: 'Failed to list files', message: String(error) });
    }
});

// GET /api/v1/files/:id - Get specific file metadata
app.get('/api/v1/files/:id', (req: Request, res: Response) => {
    try {
        const file = files.get(req.params.id);
        if (!file) {
            return res.status(404).json({ error: 'File not found' });
        }
        return res.json({ status: 'success', file });
    } catch (error) {
        return res.status(500).json({ error: 'Failed to get file', message: String(error) });
    }
});

// POST /api/v1/files/upload - Upload new file
app.post('/api/v1/files/upload', (req: Request, res: Response) => {
    try {
        const { data, name, codec } = req.body;
        
        if (!data || !name) {
            return res.status(400).json({ error: 'Missing required fields: data, name' });
        }
        
        const fileId = uuidv4();
        const originalSize = Buffer.byteLength(data, 'utf8');
        const compressedSize = originalSize * 0.564; // Simulate 56.4% compression
        const compressionRatio = compressedSize / originalSize;
        
        const metadata: FileMetadata = {
            id: fileId,
            name,
            size: originalSize,
            createdAt: new Date().toISOString(),
            updatedAt: new Date().toISOString(),
            codec: codec || 'auto',
            compressionRatio: compressionRatio
        };
        
        // Store file metadata
        files.set(fileId, metadata);
        
        // Store actual data to temp file
        const filePath = path.join(fileStorage, `${fileId}.bin`);
        fs.writeFileSync(filePath, data);
        
        return res.status(201).json({
            status: 'success',
            file: metadata,
            compressedSize: Math.round(compressedSize),
            savingsPercent: ((1 - compressionRatio) * 100).toFixed(2)
        });
    } catch (error) {
        return res.status(500).json({ error: 'Upload failed', message: String(error) });
    }
});

// ========== QUERY ENDPOINTS ==========

// POST /api/v1/query - Execute query on file
app.post('/api/v1/query', (req: Request, res: Response) => {
    try {
        const { fileId, query } = req.body;
        
        if (!fileId || !query) {
            return res.status(400).json({ error: 'Missing required fields: fileId, query' });
        }
        
        const file = files.get(fileId);
        if (!file) {
            return res.status(404).json({ error: 'File not found' });
        }
        
        const queryId = uuidv4();
        
        // Simulate query execution
        const result: QueryResult = {
            id: queryId,
            fileId,
            query,
            status: 'completed',
            resultSize: Math.floor(file.size * 0.1), // Simulate 10% result
            executionTime: Math.floor(Math.random() * 100) + 10,
            createdAt: new Date().toISOString(),
            completedAt: new Date().toISOString()
        };
        
        queries.set(queryId, result);
        
        return res.status(202).json({
            status: 'success',
            queryId,
            message: 'Query queued for processing',
            estimatedTime: '100-500ms'
        });
    } catch (error) {
        return res.status(500).json({ error: 'Query execution failed', message: String(error) });
    }
});

// GET /api/v1/query/:id - Get query result
app.get('/api/v1/query/:id', (req: Request, res: Response) => {
    try {
        const query = queries.get(req.params.id);
        if (!query) {
            return res.status(404).json({ error: 'Query not found' });
        }
        
        return res.json({
            status: 'success',
            query,
            data: {
                recordsMatched: Math.floor(Math.random() * 1000),
                columns: ['id', 'timestamp', 'value', 'category']
            }
        });
    } catch (error) {
        return res.status(500).json({ error: 'Failed to get query result', message: String(error) });
    }
});

// ========== BATCH OPERATIONS ==========

// POST /api/v1/batch/upload - Batch file upload
app.post('/api/v1/batch/upload', (req: Request, res: Response) => {
    try {
        const { files: uploadFiles } = req.body;
        
        if (!Array.isArray(uploadFiles) || uploadFiles.length === 0) {
            return res.status(400).json({ error: 'No files provided' });
        }
        
        const results = uploadFiles.map(({ data, name }: any) => {
            const fileId = uuidv4();
            const originalSize = Buffer.byteLength(data, 'utf8');
            const compressionRatio = 0.564;
            
            const metadata: FileMetadata = {
                id: fileId,
                name,
                size: originalSize,
                createdAt: new Date().toISOString(),
                updatedAt: new Date().toISOString(),
                codec: 'auto',
                compressionRatio
            };
            
            files.set(fileId, metadata);
            const filePath = path.join(fileStorage, `${fileId}.bin`);
            fs.writeFileSync(filePath, data);
            
            return { id: fileId, name, size: originalSize };
        });
        
        return res.status(201).json({
            status: 'success',
            uploaded: results.length,
            files: results
        });
    } catch (error) {
        return res.status(500).json({ error: 'Batch upload failed', message: String(error) });
    }
});

// ========== STATISTICS ==========

// GET /api/v1/stats - Get API statistics
app.get('/api/v1/stats', (_req: Request, res: Response) => {
    try {
        const fileList = Array.from(files.values());
        const totalSize = fileList.reduce((sum, f) => sum + f.size, 0);
        const totalCompressed = fileList.reduce((sum, f) => sum + (f.size * f.compressionRatio), 0);
        
        return res.json({
            status: 'success',
            files: {
                count: files.size,
                totalSize,
                totalCompressed: Math.round(totalCompressed),
                averageCompressionRatio: fileList.length > 0 
                    ? (fileList.reduce((sum, f) => sum + f.compressionRatio, 0) / fileList.length).toFixed(4)
                    : 0
            },
            queries: {
                count: queries.size,
                completed: Array.from(queries.values()).filter(q => q.status === 'completed').length,
                pending: Array.from(queries.values()).filter(q => q.status === 'pending').length
            },
            uptime: process.uptime()
        });
    } catch (error) {
        return res.status(500).json({ error: 'Failed to get statistics', message: String(error) });
    }
});

// ========== ERROR HANDLING ==========

// 404 handler for undefined routes
app.use((_req: Request, res: Response) => {
    return res.status(404).json({
        error: 'Not Found',
        message: 'The requested endpoint does not exist'
    });
});

// Error handling middleware
app.use((err: any, _req: Request, res: Response) => {
    console.error('Error:', err);
    return res.status(500).json({
        error: 'Internal Server Error',
        message: err.message
    });
});

// ========== SERVER STARTUP ==========

// Start server
app.listen(port, () => {
    console.log(`✅ Kore Cloud MVP API running on port ${port}`);
    console.log(`🚀 Base URL: http://localhost:${port}`);
    console.log(`📊 Endpoints:`);
    console.log(`   - GET    /health`);
    console.log(`   - GET    /status`);
    console.log(`   - GET    /api/v1/files`);
    console.log(`   - POST   /api/v1/files/upload`);
    console.log(`   - GET    /api/v1/files/:id`);
    console.log(`   - POST   /api/v1/query`);
    console.log(`   - GET    /api/v1/query/:id`);
    console.log(`   - POST   /api/v1/batch/upload`);
    console.log(`   - GET    /api/v1/stats`);
});
