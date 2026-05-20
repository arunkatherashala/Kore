# 📄 KORE Technical Paper - PDF Generation Guide

Complete instructions to convert the markdown paper to a **professional PDF**.

---

## 🎯 Quick Start (Choose One Method)

### Method 1: **Pandoc** (RECOMMENDED - Best Quality)

**Installation:**
```bash
# Windows (using Chocolatey)
choco install pandoc miktex

# macOS (using Homebrew)
brew install pandoc mactex

# Linux (Ubuntu/Debian)
sudo apt-get install pandoc texlive-full
```

**Convert to PDF:**
```bash
pandoc KORE_TECHNICAL_PAPER_FORMATTED.md -o KORE_Technical_Paper_v1.0.pdf \
  --pdf-engine=xelatex \
  --variable geometry:margin=1in \
  --variable linestretch=1.5 \
  --toc \
  --number-sections
```

**Result:** Professional, beautifully formatted PDF with table of contents and page numbers ✨

---

### Method 2: **Online Converter** (Easiest, No Installation)

**Step 1:** Go to https://pandoc.org/try/

**Step 2:** 
- Paste contents of `KORE_TECHNICAL_PAPER_FORMATTED.md`
- Select "From: Markdown"
- Select "To: PDF"
- Click "Convert"

**Step 3:** Download the generated PDF

**Advantage:** No installation needed, works in browser ✅

---

### Method 3: **Visual Studio Code** (For VS Code Users)

**Install extension:**
1. Open VS Code
2. Go to Extensions
3. Search: "Markdown to PDF"
4. Install "Markdown PDF" by yzane

**Convert:**
1. Open `KORE_TECHNICAL_PAPER_FORMATTED.md`
2. Right-click → "Markdown PDF: Export (pdf)"
3. Choose output location

---

### Method 4: **Python Script** (Most Customizable)

**Installation:**
```bash
pip install markdown2 pdfkit wkhtmltopdf
```

**Script:**
```python
import markdown2
import pdfkit

# Convert markdown to HTML
with open('KORE_TECHNICAL_PAPER_FORMATTED.md', 'r') as f:
    md_text = f.read()

html = markdown2.markdown(md_text, extras=['tables', 'fenced-code-blocks'])

# Add styling
html = f"""
<html>
<head>
<style>
    body {{ font-family: 'Calibri', sans-serif; line-height: 1.6; }}
    h1 {{ color: #1a73e8; border-bottom: 2px solid #1a73e8; padding-bottom: 10px; }}
    h2 {{ color: #34a853; margin-top: 30px; }}
    code {{ background-color: #f5f5f5; padding: 2px 5px; }}
    table {{ border-collapse: collapse; width: 100%; }}
    th, td {{ border: 1px solid #ddd; padding: 8px; text-align: left; }}
    th {{ background-color: #f0f0f0; }}
</style>
</head>
<body>
{html}
</body>
</html>
"""

# Convert to PDF
pdfkit.from_string(html, 'KORE_Technical_Paper_v1.0.pdf')
print("✅ PDF generated: KORE_Technical_Paper_v1.0.pdf")
```

**Run:**
```bash
python convert_to_pdf.py
```

---

## 📋 Step-by-Step Guide (Pandoc - Recommended)

### Step 1: Install Pandoc

**Windows (Chocolatey):**
```powershell
choco install pandoc miktex
```

**macOS (Homebrew):**
```bash
brew install pandoc mactex
```

**Linux (Ubuntu/Debian):**
```bash
sudo apt-get update
sudo apt-get install pandoc texlive-full texlive-fonts-recommended
```

### Step 2: Prepare Document

Ensure `KORE_TECHNICAL_PAPER_FORMATTED.md` is in your project directory.

### Step 3: Create Conversion Script

**PowerShell (Windows):**
```powershell
# create_pdf.ps1
$inputFile = "KORE_TECHNICAL_PAPER_FORMATTED.md"
$outputFile = "KORE_Technical_Paper_v1.0.pdf"

pandoc $inputFile -o $outputFile `
  --pdf-engine=xelatex `
  --variable geometry:margin=1in `
  --variable linestretch=1.5 `
  --toc `
  --number-sections `
  --variable pagestyle=headings

Write-Output "✅ PDF created: $outputFile"
```

**Bash (macOS/Linux):**
```bash
#!/bin/bash
# create_pdf.sh

INPUT="KORE_TECHNICAL_PAPER_FORMATTED.md"
OUTPUT="KORE_Technical_Paper_v1.0.pdf"

pandoc "$INPUT" -o "$OUTPUT" \
  --pdf-engine=xelatex \
  --variable geometry:margin=1in \
  --variable linestretch=1.5 \
  --toc \
  --number-sections \
  --variable pagestyle=headings

echo "✅ PDF created: $OUTPUT"
```

### Step 4: Run the Script

**Windows:**
```powershell
.\create_pdf.ps1
```

**macOS/Linux:**
```bash
chmod +x create_pdf.sh
./create_pdf.sh
```

### Step 5: Verify Output

Check that `KORE_Technical_Paper_v1.0.pdf` was created successfully.

---

## 🎨 Customization Options

### Option 1: Change Font

```bash
pandoc input.md -o output.pdf \
  --pdf-engine=xelatex \
  --variable mainfont="Georgia" \
  --variable monofont="Courier New"
```

### Option 2: Add Cover Page

Create `cover.md`:
```markdown
---
title: KORE Technical Paper
subtitle: v1.2.0 - Production Ready
author: Arun Kather Ashala
date: May 20, 2026
---
```

Then:
```bash
pandoc cover.md KORE_TECHNICAL_PAPER_FORMATTED.md -o output.pdf --pdf-engine=xelatex
```

### Option 3: Change Colors

```bash
pandoc input.md -o output.pdf \
  -V colorlinks \
  -V urlcolor=NavyBlue \
  -V linkcolor=NavyBlue \
  -V pdf-engine=xelatex
```

### Option 4: Different Margins

```bash
pandoc input.md -o output.pdf \
  -V geometry:"top=2cm,bottom=2cm,left=2cm,right=2cm" \
  --pdf-engine=xelatex
```

---

## 🔧 Troubleshooting

### Issue: Pandoc not found

**Solution:**
```bash
# Check if installed
which pandoc  # macOS/Linux
where pandoc  # Windows

# If not, reinstall
brew install pandoc  # macOS
choco install pandoc  # Windows
```

### Issue: LaTeX errors

**Solution:**
```bash
# Install full LaTeX distribution
brew cask install mactex  # macOS
# or in Windows (miktex installer)
```

### Issue: PDF looks wrong

**Solution:** Use `--pdf-engine=xelatex` instead of default

### Issue: Special characters not rendering

**Solution:** Ensure file is saved as UTF-8 encoding

---

## 📊 Final PDF Specification

The generated PDF will include:

✅ **Professional formatting:**
- Calibri font for body text
- Courier New for code blocks
- 1-inch margins all around
- 1.5 line spacing
- Proper heading hierarchy

✅ **Automatic features:**
- Table of contents (clickable)
- Page numbers
- Section numbering
- Hyperlinked references
- Professional headers/footers

✅ **Content structure:**
- Cover page (title, author, date)
- Table of contents
- 11 main sections
- Appendices
- Citations
- Professional footer

**Result:** Enterprise-grade PDF ready for distribution! 🎉

---

## 🚀 Quick Commands Reference

### Pandoc Basic
```bash
pandoc input.md -o output.pdf
```

### Pandoc Professional
```bash
pandoc input.md -o output.pdf \
  --pdf-engine=xelatex \
  --variable geometry:margin=1in \
  --variable linestretch=1.5 \
  --toc \
  --number-sections
```

### Pandoc with Cover
```bash
pandoc cover.md input.md -o output.pdf \
  --pdf-engine=xelatex \
  --toc
```

### Pandoc Custom Styling
```bash
pandoc input.md -o output.pdf \
  --pdf-engine=xelatex \
  --template=template.latex \
  --variable fontsize=11pt \
  --variable geometry:margin=1.25in
```

---

## 📱 Recommended Publication Formats

### For LinkedIn
- Export as PDF
- Upload directly to post

### For Email
- PDF file (1-5MB typical)
- Include link to GitHub in email body

### For Website
- Embed on GitHub Releases page
- Host on project website
- Reference in documentation

### For Printing
- Use professional format
- Ensure margins are correct
- Test print preview before printing

---

## ✅ Verification Checklist

After generating PDF, verify:

- ✅ PDF opens without errors
- ✅ All pages render correctly
- ✅ Table of contents is clickable
- ✅ Code blocks are formatted properly
- ✅ Tables display correctly
- ✅ Page numbers are present
- ✅ Headers/footers appear
- ✅ All images/diagrams display (if any)
- ✅ File size is reasonable (<10MB)
- ✅ Can be printed without issues

---

## 🎁 Pre-made Bash Script

Save as `generate_pdf.sh`:

```bash
#!/bin/bash

# KORE Technical Paper PDF Generator
# This script generates a professional PDF from markdown

set -e

INPUT_FILE="KORE_TECHNICAL_PAPER_FORMATTED.md"
OUTPUT_FILE="KORE_Technical_Paper_v1.0.pdf"

echo "🚀 Starting PDF generation..."

# Check if pandoc is installed
if ! command -v pandoc &> /dev/null; then
    echo "❌ Pandoc not found. Install with:"
    echo "   macOS: brew install pandoc mactex"
    echo "   Linux: sudo apt-get install pandoc texlive-full"
    echo "   Windows: choco install pandoc miktex"
    exit 1
fi

echo "📝 Input file: $INPUT_FILE"
echo "📄 Output file: $OUTPUT_FILE"

# Generate PDF
pandoc "$INPUT_FILE" -o "$OUTPUT_FILE" \
  --pdf-engine=xelatex \
  --variable geometry:margin=1in \
  --variable linestretch=1.5 \
  --toc \
  --number-sections \
  --variable pagestyle=headings \
  -f markdown+pipe_tables

echo "✅ PDF generated successfully!"
echo "📊 File info:"
ls -lh "$OUTPUT_FILE"
echo ""
echo "🎉 Ready to share on:"
echo "   - LinkedIn"
echo "   - GitHub Releases"
echo "   - Email"
echo "   - Websites"
```

**Run:**
```bash
chmod +x generate_pdf.sh
./generate_pdf.sh
```

---

## 💡 Pro Tips

1. **Version Control:** Keep both `.md` and `.pdf` in GitHub
   ```bash
   git add KORE_TECHNICAL_PAPER_FORMATTED.md
   git add KORE_Technical_Paper_v1.0.pdf
   git commit -m "docs: Add v1.0 technical paper (PDF)"
   ```

2. **Release Integration:** Add PDF to GitHub releases
   ```bash
   gh release upload v1.2.0 KORE_Technical_Paper_v1.0.pdf
   ```

3. **Website Integration:** Host on ReadTheDocs or similar

4. **Email Distribution:** Attach PDF to press releases

5. **LinkedIn:** Use document upload feature for better reach

---

**Status:** ✅ Ready to convert to PDF!

**Next Step:** Choose your preferred method above and generate the PDF.

All methods will produce a **professional, enterprise-grade PDF** suitable for:
- 📧 Email distribution
- 📱 LinkedIn sharing
- 🌐 Website hosting
- 🏢 Enterprise presentations
- 📚 Technical documentation
