# 🎯 KORE v1.2.3 Slack Webhook Setup - Final Steps

## Status: Ready for Quick 5-Minute Completion ✅

You're **99% done**! Here's exactly what you need to do to activate Slack alerts:

---

## 📋 Step-by-Step Setup

### Step 1: Create a Channel in Your Slack Workspace (2 minutes)

1. Open Slack: **https://app.slack.com/client/T0B5JU3BWCF**
2. Click **+** next to "Channels" on the left sidebar
3. Click **"Create a channel"**
4. Name it: `kore-monitoring` (or any name you prefer)
5. Click **"Create"**

**Done!** Now you have a channel ready for webhooks.

---

### Step 2: Generate Your Slack Webhook URL (2 minutes)

1. Go to: **https://api.slack.com/apps/A0B5YBJ5TC6/incoming-webhooks**
2. Make sure the toggle is **ON** (green) ✓ (already done!)
3. Click **"Add New Webhook to Workspace"**
4. Choose your workspace: **"New Workspace"**
5. Select channel: **#kore-monitoring** (or your channel name)
6. Click **"Allow"**
7. **COPY** the webhook URL that appears (looks like: `https://hooks.slack.com/services/T.../B.../XXXXX`)

---

### Step 3: Update Google Apps Script (1 minute)

1. Go to: **https://script.google.com/u/0/home/projects/1PglBch3L7cdpkOCXJL4GLXfzvOIu2xw7DhauSOMkg3CR12VxNYlbU0P8/edit**
2. Find line 31 - the `sendSlackAlert` function
3. Replace this line:
   ```javascript
   const slackWebhook = "https://hooks.slack.com/services/YOUR/SLACK/WEBHOOK";
   ```
   
   With your actual webhook (paste it):
   ```javascript
   const slackWebhook = "https://hooks.slack.com/services/T0B5JU3.../B0B5YBJ.../XXXXX";
   ```

4. **Save** with Ctrl+S (or click Deploy)
5. Done! ✅

---

### Step 4: Test the Integration (optional, 1 minute)

1. In Apps Script, click the **Run** button dropdown
2. Select **`checkAnomalies`** function
3. Click **Run**
4. Check your Slack channel - you should see a test alert!

---

## 🔗 Your Live Dashboard URLs

**Share these with stakeholders now:**

- **📊 Monitoring Hub** (Start here)
  - File: `projects/community-platform/website/monitoring-hub.html`
  - Shows all dashboards + live metrics

- **📈 Standard Dashboard** (Real-time data)
  - File: `projects/community-platform/website/monitoring.html`
  - Hourly metrics, 4 charts, metrics table

- **🔮 Advanced Dashboard** (Predictions)
  - File: `projects/community-platform/website/advanced-dashboard.html`
  - 7-day forecasts, anomaly detection, exports

- **🏠 Homepage** (Updated)
  - File: `projects/community-platform/website/index.html`
  - Now features live monitoring showcase

---

## 🎁 Current System Status

### ✅ Working Right Now
- Email alerts → Tested ✓
- Hourly data collection → Active ✓
- 12-platform tracking → Working ✓
- Dashboards → All deployed ✓
- Predictions/Analytics → Verified ✓

### ⏳ Pending (5 minutes to complete)
- Slack webhook → Replace placeholder with your URL
- Weekly triggers → Optional (can set up anytime)

---

## 📧 Reference: Current Alert Functions

Your Apps Script has these ready-to-use functions:

```javascript
// Email alerts - ALREADY WORKING ✅
sendEmailAlert(subject, message)
  → Sends to: arunkatherashala@gmail.com
  → Status: TESTED & VERIFIED

// Slack alerts - READY, UPDATE WEBHOOK
sendSlackAlert(message, severity)
  → Severity: "critical" (red), "warning" (orange), "info" (green)
  → Status: READY FOR WEBHOOK

// Anomaly detection - ACTIVE
checkAnomalies()
  → Detects Maven downloads >5% drop
  → Triggers both email + Slack

// Weekly reports - AVAILABLE
sendWeeklyReport()
  → Generates weekly summary
  → Status: Ready for scheduling
```

---

## 🚀 Quick Reference

| Item | URL | Status |
|------|-----|--------|
| Slack Webhook Setup | https://api.slack.com/apps/A0B5YBJ5TC6/incoming-webhooks | ⏳ Update webhook |
| Apps Script Code | https://script.google.com/u/0/home/projects/1PglBch3L7cdpkOCXJL4GLXfzvOIu2xw7DhauSOMkg3CR12VxNYlbU0P8/edit | 🟢 Ready |
| Google Sheet | https://docs.google.com/spreadsheets/d/18nQYwSUyz0uZqSrvwbXQL3YUHnrQNdxzLTj_qo9rETs/ | 🟢 Live |
| Slack Workspace | https://app.slack.com/client/T0B5JU3BWCF | 🟢 Ready |

---

## 📞 Troubleshooting

**"Webhook not working"**
- ✓ Check that the URL starts with `https://hooks.slack.com/services/`
- ✓ Make sure there are no extra spaces or characters
- ✓ Verify the channel still exists in your workspace

**"Email works but Slack doesn't"**
- ✓ The webhook URL is probably still a placeholder
- ✓ Follow Step 2 & 3 above to replace it

**"I don't see the webhook URL generated"**
- ✓ Make sure you selected a channel (#kore-monitoring)
- ✓ The "Allow" button becomes enabled once you select a channel

---

## ✨ What Happens After Setup

Once you add your webhook URL:

1. **Every hour**: Data refreshes automatically
2. **Maven drops >5%**: Slack alert + email
3. **Every Thursday 9 AM**: Weekly summary emails
4. **24/7 Monitoring**: All dashboards stay live

---

## 🎊 Success Indicators

Once complete, you'll see:
- ✅ Green checkmark on your Slack app
- ✅ Test messages appearing in #kore-monitoring
- ✅ Emails still arriving from Google Apps Script
- ✅ All dashboards displaying live data

---

**Time to completion: ~5 minutes**
**Difficulty: Easy** (just copy-paste the webhook URL)
**Impact: Full 24/7 automated monitoring + alerts** 🚀
