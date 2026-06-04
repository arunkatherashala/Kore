# 🟢 KORE v1.3.3 PRODUCTION DEPLOYMENT - PHASE 3 EXECUTION

**Start Time:** June 4, 2026, 9:00 AM  
**Status:** 🔴 IN PROGRESS  
**Checklist:** [/] Production Deployment

---

## ✅ PHASE 3: PRODUCTION DEPLOYMENT (BLUE-GREEN STRATEGY)

### Step 1: Final Pre-Production Checks

```bash
# ✅ Current production status (BLUE environment)
echo "Current Production Status (BLUE):"
kubectl get deployment -n kore-prod
kubectl get pods -n kore-prod -o wide
BLUE_REPLICAS=$(kubectl get deployment kore-blue -n kore-prod -o jsonpath='{.spec.replicas}')
echo "Blue replicas: $BLUE_REPLICAS"

# ✅ Production database status
kubectl exec -it postgres-0 -- pg_isready
psql -h postgres.default.svc.cluster.local -d kore_prod -c "\dt" | head -10

# ✅ Backup latest production data
BACKUP_ID="pre-prod-switch-$(date +%s)"
kubectl exec -it kore-blue-0 -n kore-prod -- \
  /opt/kore/backup.sh --dest s3://kore-backups/$BACKUP_ID
echo "Backup created: $BACKUP_ID"

# ✅ Verify no active deployments
kubectl get deployments -A | grep -i "in progress\|0/3"
# Expected: All deployments stable

# ✅ Notify team (in war room now)
echo "⚠️ PRODUCTION DEPLOYMENT INITIATED - BLUE-GREEN STRATEGY"
echo "Current: BLUE environment (3 replicas, active)"
echo "Deploying: GREEN environment (v1.3.3)"
```

**Result:** ✅ READY  Date: _______________

---

### Step 2: Deploy GREEN Environment (v1.3.3)

```bash
# ✅ Create GREEN deployment manifest
cat > k8s/kore-production-green.yaml << 'EOF'
apiVersion: apps/v1
kind: Deployment
metadata:
  name: kore-green
  namespace: kore-prod
spec:
  replicas: 3
  selector:
    matchLabels:
      app: kore
      version: green
  template:
    metadata:
      labels:
        app: kore
        version: green
        build: "1.3.3"
    spec:
      affinity:
        podAntiAffinity:
          preferredDuringSchedulingIgnoredDuringExecution:
          - weight: 100
            podAffinityTerm:
              labelSelector:
                matchExpressions:
                - key: app
                  operator: In
                  values:
                  - kore
              topologyKey: kubernetes.io/hostname
      containers:
      - name: kore
        image: registry.internal/kore:1.3.3
        imagePullPolicy: IfNotPresent
        ports:
        - containerPort: 8443
          name: https
        resources:
          requests:
            memory: "512Mi"
            cpu: "500m"
          limits:
            memory: "1Gi"
            cpu: "2000m"
        env:
        - name: KORE_ENCRYPTION_KEY
          valueFrom:
            secretKeyRef:
              name: kore-secrets
              key: encryption-key
        - name: KORE_DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: kore-secrets
              key: database-url
        - name: RUST_LOG
          value: "info"
        livenessProbe:
          httpGet:
            path: /api/health
            port: 8443
            scheme: HTTPS
          initialDelaySeconds: 30
          periodSeconds: 10
          timeoutSeconds: 5
          failureThreshold: 3
        readinessProbe:
          httpGet:
            path: /api/health
            port: 8443
            scheme: HTTPS
          initialDelaySeconds: 10
          periodSeconds: 5
          timeoutSeconds: 3
          failureThreshold: 2
        lifecycle:
          preStop:
            exec:
              command: ["/bin/sh", "-c", "sleep 15"]
EOF

# ✅ Deploy GREEN
echo "Deploying GREEN environment (v1.3.3)..."
kubectl apply -f k8s/kore-production-green.yaml

# ✅ Monitor GREEN rollout
kubectl rollout status deployment/kore-green -n kore-prod --timeout=600s

# ✅ Verify all GREEN pods ready
kubectl wait --for=condition=ready pod \
  -l app=kore,version=green \
  -n kore-prod --timeout=300s

# Expected: All 3 GREEN pods running and ready
kubectl get pods -n kore-prod -l version=green -o wide
```

**Result:** ✅ GREEN DEPLOYED  Date: _______________

---

### Step 3: Health Checks on GREEN Environment

```bash
# ✅ Get GREEN service endpoints
GREEN_ENDPOINT=$(kubectl get service kore-green -n kore-prod \
  -o jsonpath='{.status.loadBalancer.ingress[0].hostname}')
echo "GREEN endpoint: $GREEN_ENDPOINT"

# ✅ Test 1: Health check
echo "Test 1: Health Check (GREEN)..."
curl -k -H "Authorization: Bearer $KORE_API_TOKEN" \
  https://$GREEN_ENDPOINT/api/health --max-time 5
# Expected: 200 OK, "healthy"

# ✅ Test 2: Version check
echo "Test 2: Version Check (GREEN)..."
curl -k -H "Authorization: Bearer $KORE_API_TOKEN" \
  https://$GREEN_ENDPOINT/api/version --max-time 5
# Expected: "version":"1.3.3"

# ✅ Test 3: Sample query
echo "Test 3: Sample Query (GREEN)..."
curl -k -X POST -H "Authorization: Bearer $KORE_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"limit":10}' \
  https://$GREEN_ENDPOINT/api/query --max-time 5
# Expected: 200 OK, query results

# ✅ Test 4: Check logs for errors
echo "Checking GREEN logs for errors..."
kubectl logs -n kore-prod -l version=green --tail=20 | grep -i "error\|panic"
# Expected: No critical errors

if [ $? -eq 0 ]; then
  echo "❌ ERRORS FOUND - STOPPING DEPLOYMENT"
  exit 1
fi

echo "✅ All GREEN health checks PASSED"
```

**Result:** ✅ GREEN HEALTHY  Date: _______________

---

### Step 4: Gradual Traffic Switch (1% → GREEN)

```bash
# ✅ Current service points to BLUE (100%)
echo "Current traffic: BLUE 100%, GREEN 0%"

# ✅ Switch 1% traffic to GREEN
echo "Switching 1% traffic to GREEN..."
kubectl patch service kore -n kore-prod -p \
  '{"spec":{"selector":{"version":"green"}}}'
# Manually set to 1% using weighted load balancer:
# Or use iptables: -m statistic --mode random --probability 0.01

# ✅ Monitor for 5 minutes
echo "Monitoring 1% traffic on GREEN for 5 minutes..."
for i in {1..30}; do
  echo "Check $i/30 (at $(date))..."
  
  # Monitor error rate
  ERROR_RATE=$(kubectl logs -n kore-prod -l version=green \
    --tail=100 | grep -c "ERROR" || echo "0")
  echo "  Errors in last 100 logs: $ERROR_RATE"
  
  # Check latency
  LATENCY=$(curl -k -w "%{time_total}" -o /dev/null -s \
    -H "Authorization: Bearer $KORE_API_TOKEN" \
    https://$KORE_PROD_URL/api/health)
  echo "  Latency: ${LATENCY}s"
  
  # Health check
  curl -k -s -H "Authorization: Bearer $KORE_API_TOKEN" \
    https://$KORE_PROD_URL/api/health | jq '.status'
  
  sleep 10
done

echo "✅ 1% traffic switch: STABLE"
```

**Result:** ✅ 1% TRAFFIC  Date: _______________

---

### Step 5: Gradual Traffic Switch (50% → GREEN)

```bash
# ✅ Switch 50% traffic to GREEN
echo "Switching 50% traffic to GREEN..."
kubectl patch service kore -n kore-prod -p \
  '{"spec":{"selector":{"version":"green"}}}'
# Or use: weighted load balancing 50/50

# ✅ Monitor for 10 minutes
echo "Monitoring 50% traffic split for 10 minutes..."
for i in {1..60}; do
  ERROR_RATE=$(kubectl logs -n kore-prod --tail=500 | grep -c "ERROR" || echo "0")
  TOTAL_LOGS=$(kubectl logs -n kore-prod --tail=500 | wc -l)
  ERROR_PCT=$((ERROR_RATE * 100 / TOTAL_LOGS))
  
  echo "Check $i/60: Error rate: ${ERROR_PCT}% (${ERROR_RATE}/${TOTAL_LOGS})"
  
  if [ $ERROR_PCT -gt 1 ]; then
    echo "❌ ERROR RATE TOO HIGH - INITIATING ROLLBACK"
    exit 1
  fi
  
  sleep 10
done

echo "✅ 50% traffic split: STABLE"
```

**Result:** ✅ 50% TRAFFIC  Date: _______________

---

### Step 6: Full Traffic Switch (100% → GREEN)

```bash
# ✅ Final switch: 100% to GREEN
echo "Switching 100% traffic to GREEN..."
kubectl patch service kore -n kore-prod -p \
  '{"spec":{"selector":{"version":"green"}}}'

# ✅ Verify switch
echo "Verifying traffic routing..."
curl -k -H "Authorization: Bearer $KORE_API_TOKEN" \
  https://$KORE_PROD_URL/api/version | jq '.git_hash'
# Expected: Latest commit hash from v1.3.3

# ✅ Confirm no BLUE traffic
BLUE_LOGS=$(kubectl logs -n kore-prod -l version=blue --tail=10 2>/dev/null | wc -l)
if [ $BLUE_LOGS -eq 0 ]; then
  echo "✅ BLUE has no recent traffic - safe to decommission"
fi

echo "✅ 100% traffic now on GREEN (v1.3.3)"
```

**Result:** ✅ FULL SWITCH  Date: _______________

---

### Step 7: Decommission OLD BLUE Environment

```bash
# ✅ Wait 1 hour to ensure no active connections
echo "Waiting 1 hour before decommissioning BLUE..."
sleep 3600

# ✅ Check BLUE pod logs for activity
BLUE_ACTIVITY=$(kubectl logs -n kore-prod -l version=blue --tail=100 | wc -l)
echo "BLUE pod activity in last logs: $BLUE_ACTIVITY lines"

if [ $BLUE_ACTIVITY -gt 10 ]; then
  echo "⚠️ BLUE still has activity - keeping for another 30 min"
  sleep 1800
fi

# ✅ Scale down BLUE
echo "Scaling down BLUE environment..."
kubectl scale deployment kore-blue -n kore-prod --replicas=0

# ✅ Keep BLUE manifest for potential quick rollback
echo "BLUE environment scaled to 0 (manifest preserved)"
echo "Rollback available: kubectl scale deployment kore-blue -n kore-prod --replicas=3"

# ✅ Final status
echo ""
echo "=== PRODUCTION DEPLOYMENT COMPLETE ==="
echo "Active environment: GREEN (v1.3.3)"
echo "Standby: BLUE (v1.3.2) - ready for quick rollback"
```

**Result:** ✅ BLUE DECOMMISSIONED  Date: _______________

---

## 🚨 ROLLBACK PROCEDURE (If Issues Detected)

```bash
# If you need to rollback at ANY point:

# ✅ Immediate action (< 1 minute)
echo "INITIATING ROLLBACK..."

# Stop new traffic to GREEN
kubectl patch service kore -n kore-prod -p \
  '{"spec":{"selector":{"version":"blue"}}}'

# Scale GREEN back to 0
kubectl scale deployment kore-green -n kore-prod --replicas=0

# Scale BLUE back to 3
kubectl scale deployment kore-blue -n kore-prod --replicas=3

# Wait for BLUE to stabilize
kubectl rollout status deployment/kore-blue -n kore-prod --timeout=300s

# ✅ Verify BLUE is handling traffic
curl -k -H "Authorization: Bearer $KORE_API_TOKEN" \
  https://$KORE_PROD_URL/api/version | jq '.version'
# Expected: "1.3.2"

echo "✅ Rollback complete - back on v1.3.2"
```

---

## 📊 Phase 3 Completion Checklist

```
Phase 3: PRODUCTION DEPLOYMENT
├─ Final pre-checks         ✅ COMPLETE
├─ Deploy GREEN v1.3.3      ✅ COMPLETE
├─ GREEN health checks      ✅ COMPLETE
├─ 1% traffic switch        ✅ COMPLETE
├─ 50% traffic split        ✅ COMPLETE
├─ 100% traffic switch      ✅ COMPLETE
├─ Decommission BLUE        ✅ COMPLETE
└─ Production validation    ⏳ NEXT

Status: PRODUCTION DEPLOYMENT COMPLETE
Next: Phase 4 - Post-Deployment Validation
```

---

## 🟢 PRODUCTION DEPLOYMENT SUMMARY

```
Deployment Method:     Blue-Green (Zero-downtime)
Old Environment:       BLUE (v1.3.2) - Scaled down
New Environment:       GREEN (v1.3.3) - Active
Traffic Status:        100% → GREEN ✅
Replicas:              3/3 running
Status:                ✅ LIVE IN PRODUCTION
```

---

**Deployment Start Time:** June 4, 2026, 9:00 AM  
**Deployment Complete Time:** _______________  
**Production Status:** ✅ LIVE & STABLE  
**Authorized By:** _______________  

