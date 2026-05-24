import express, { Express, Request, Response } from 'express';
import cors from 'cors';
import helmet from 'helmet';
import compression from 'compression';
import dotenv from 'dotenv';

dotenv.config();

const app: Express = express();
const port = process.env.PORT || 3000;

// Middleware
app.use(helmet());
app.use(cors());
app.use(compression());
app.use(express.json({ limit: '50mb' }));
app.use(express.urlencoded({ limit: '50mb', extended: true }));

// Health check endpoint
app.get('/health', (req: Request, res: Response) => {
    res.json({
        status: 'ok',
        timestamp: new Date().toISOString(),
        version: '1.0.0'
    });
});

// Status endpoint
app.get('/status', (req: Request, res: Response) => {
    res.json({
        service: 'kore-cloud-mvp',
        status: 'running',
        uptime: process.uptime(),
        environment: process.env.NODE_ENV || 'development',
        database: 'connecting...',
        s3: 'connecting...'
    });
});

// API v1 routes (placeholder)
app.get('/api/v1/files', (req: Request, res: Response) => {
    res.json({
        message: 'File listing endpoint',
        files: [],
        total: 0
    });
});

app.post('/api/v1/files/upload', (req: Request, res: Response) => {
    res.status(201).json({
        message: 'File upload endpoint',
        id: 'placeholder-id'
    });
});

app.post('/api/v1/query', (req: Request, res: Response) => {
    res.json({
        message: 'Query execution endpoint',
        query_id: 'placeholder-query-id'
    });
});

// Error handling middleware
app.use((err: any, req: Request, res: Response) => {
    console.error('Error:', err);
    res.status(500).json({
        error: 'Internal Server Error',
        message: err.message
    });
});

// 404 handler
app.use((req: Request, res: Response) => {
    res.status(404).json({
        error: 'Not Found',
        path: req.path
    });
});

app.listen(port, () => {
    console.log(`🚀 Kore Cloud MVP API running on http://localhost:${port}`);
    console.log(`📝 Environment: ${process.env.NODE_ENV || 'development'}`);
    console.log(`🗄️  Database: ${process.env.DB_HOST}:${process.env.DB_PORT}/${process.env.DB_NAME}`);
    console.log(`📦 S3 Endpoint: ${process.env.AWS_ENDPOINT}`);
});
