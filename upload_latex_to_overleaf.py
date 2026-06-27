#!/usr/bin/env python3
"""
Automate LaTeX upload and PDF generation on Overleaf
This script:
1. Opens the Overleaf project
2. Replaces the content with KORE_TECHNICAL_PAPER.tex
3. Triggers recompilation
4. Downloads the PDF
"""

import time
import os
from pathlib import Path
from selenium import webdriver
from selenium.webdriver.common.by import By
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.common.action_chains import ActionChains
from selenium.webdriver.support.ui import WebDriverWait
from selenium.webdriver.support import expected_conditions as EC
from selenium.webdriver.chrome.options import Options

# Configuration
OVERLEAF_PROJECT_URL = "https://www.overleaf.com/project/6a0dcf1ab1e5aa5787d8ec50"
LATEX_FILE = r"c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\KORE_TECHNICAL_PAPER.tex"
DOWNLOAD_DIR = r"c:\Users\ksak_\OneDrive\Desktop\dbt_prep\Kore\PDFs"

def setup_driver():
    """Setup Selenium WebDriver"""
    print("🚀 Setting up Chrome WebDriver...")
    
    # Create download directory if it doesn't exist
    Path(DOWNLOAD_DIR).mkdir(parents=True, exist_ok=True)
    
    chrome_options = Options()
    # Don't use headless mode so we can see what's happening
    # chrome_options.add_argument("--headless")
    chrome_options.add_argument("--no-sandbox")
    chrome_options.add_argument("--disable-dev-shm-usage")
    
    # Set download directory
    prefs = {
        "download.default_directory": DOWNLOAD_DIR,
        "download.prompt_for_download": False,
        "profile.default_content_settings.popups": 0
    }
    chrome_options.add_experimental_option("prefs", prefs)
    
    driver = webdriver.Chrome(options=chrome_options)
    return driver

def read_latex_file():
    """Read the LaTeX file content"""
    print(f"📄 Reading LaTeX file: {LATEX_FILE}")
    with open(LATEX_FILE, 'r', encoding='utf-8') as f:
        content = f.read()
    print(f"✅ Read {len(content)} characters")
    return content

def open_overleaf_project(driver):
    """Open the Overleaf project"""
    print(f"\n🌐 Opening Overleaf project...")
    driver.get(OVERLEAF_PROJECT_URL)
    
    # Wait for the editor to load
    print("⏳ Waiting for editor to load...")
    try:
        WebDriverWait(driver, 30).until(
            EC.presence_of_element_located((By.CSS_SELECTOR, ".cm-content, [role='textbox'], textarea"))
        )
        print("✅ Editor loaded!")
    except Exception as e:
        print(f"⚠️ Warning: Editor may not have loaded properly: {e}")
    
    time.sleep(2)

def click_editor(driver):
    """Click on the editor to focus it"""
    print("\n🎯 Focusing editor...")
    
    # Try multiple selectors
    selectors = [
        ".cm-content",
        ".cm-editor",
        "[role='textbox']",
        "textarea",
        ".ace_editor",
        ".CodeMirror"
    ]
    
    editor = None
    for selector in selectors:
        try:
            elements = driver.find_elements(By.CSS_SELECTOR, selector)
            if elements:
                editor = elements[0]
                # Use ActionChains for more reliable clicking
                actions = ActionChains(driver)
                actions.move_to_element(editor).click().perform()
                print(f"✅ Clicked editor using selector: {selector}")
                time.sleep(1)
                return True
        except Exception as e:
            continue
    
    print("⚠️ Could not find editor element, trying body click...")
    # Fallback: click body multiple times
    body = driver.find_element(By.TAG_NAME, "body")
    actions = ActionChains(driver)
    actions.move_to_element(body).click().perform()
    time.sleep(1)
    return True

def select_all_and_delete(driver):
    """Select all content and delete it"""
    print("\n🗑️  Selecting all content...")
    
    # Click in editor first
    body = driver.find_element(By.TAG_NAME, "body")
    actions = ActionChains(driver)
    actions.click(body).perform()
    time.sleep(0.5)
    
    # Select all with Ctrl+A
    actions = ActionChains(driver)
    actions.key_down(Keys.CONTROL).send_keys("a").key_up(Keys.CONTROL).perform()
    time.sleep(1)
    print("✅ Selected all (Ctrl+A)")
    
    # Delete
    actions = ActionChains(driver)
    actions.send_keys(Keys.DELETE).perform()
    time.sleep(1)
    print("✅ Deleted old content")

def paste_latex(driver, latex_content):
    """Paste the LaTeX content"""
    print("\n📝 Pasting LaTeX content...")
    
    # Copy to Windows clipboard using PowerShell
    import subprocess
    
    # First, setup clipboard
    print("📋 Copying content to clipboard...")
    ps_cmd = f'''
    $content = @"
{latex_content}
"@
    $content | Set-Clipboard
    '''
    
    try:
        result = subprocess.run(
            ["powershell", "-NoProfile", "-Command", ps_cmd],
            check=True,
            capture_output=True,
            text=True,
            timeout=10
        )
        print("✅ Content copied to clipboard")
    except Exception as e:
        print(f"⚠️ Clipboard copy failed: {e}")
        return False
    
    time.sleep(1)
    
    # Click in the editor
    body = driver.find_element(By.TAG_NAME, "body")
    actions = ActionChains(driver)
    actions.click(body).perform()
    time.sleep(0.5)
    
    # Paste using Ctrl+V
    actions = ActionChains(driver)
    actions.key_down(Keys.CONTROL).send_keys("v").key_up(Keys.CONTROL).perform()
    print("✅ Pasted LaTeX (Ctrl+V)")
    
    # Wait for paste to complete
    time.sleep(5)
    return True

def trigger_recompile(driver):
    """Click the Recompile button"""
    print("\n🔄 Triggering recompilation...")
    
    try:
        # Wait for recompile button to be present
        WebDriverWait(driver, 10).until(
            EC.presence_of_element_located((By.XPATH, 
                "//button[contains(., 'Recompile')] | //button[contains(., 'Compile')]"))
        )
        
        # Find and click the Recompile button using ActionChains
        recompile_buttons = driver.find_elements(By.XPATH, 
            "//button[contains(., 'Recompile')] | //button[contains(., 'Compile')]"
        )
        
        if recompile_buttons:
            actions = ActionChains(driver)
            actions.move_to_element(recompile_buttons[0]).click().perform()
            print("✅ Clicked Recompile button")
            time.sleep(2)
        else:
            raise Exception("No recompile button found")
            
    except Exception as e:
        print(f"⚠️ Could not find recompile button: {e}")
        print("⏳ Waiting anyway for auto-compile...")
        time.sleep(3)

def wait_for_pdf(driver, timeout=60):
    """Wait for PDF to compile"""
    print(f"\n⏳ Waiting for PDF compilation (max {timeout}s)...")
    
    start_time = time.time()
    
    while time.time() - start_time < timeout:
        try:
            # Check if PDF is visible
            pdf_elements = driver.find_elements(By.CSS_SELECTOR, "iframe[src*='pdf'], canvas")
            if pdf_elements:
                print("✅ PDF compiled and visible!")
                return True
        except:
            pass
        
        time.sleep(2)
        print("⏳ Still waiting...", end="\r")
    
    print("\n⚠️ PDF compilation timeout, continuing anyway...")
    return False

def download_pdf(driver):
    """Download the PDF"""
    print("\n⬇️  Downloading PDF...")
    
    try:
        # Find download button
        download_buttons = driver.find_elements(By.XPATH, 
            "//button[contains(@aria-label, 'PDF')] | //button[contains(text(), 'PDF')] | //a[contains(@href, 'pdf')]"
        )
        
        if download_buttons:
            download_buttons[0].click()
            print("✅ Clicked download button")
        else:
            print("⚠️ Download button not found, using keyboard shortcut...")
            # Try Ctrl+S to save
            driver.find_element(By.TAG_NAME, "body").send_keys(Keys.CONTROL + "s")
            print("✅ Triggered save (Ctrl+S)")
        
        # Wait for download
        time.sleep(5)
        
        # Check if file was downloaded
        pdf_files = list(Path(DOWNLOAD_DIR).glob("*.pdf"))
        if pdf_files:
            latest_pdf = max(pdf_files, key=os.path.getctime)
            print(f"✅ PDF downloaded: {latest_pdf}")
            return str(latest_pdf)
    except Exception as e:
        print(f"⚠️ Error during download: {e}")
    
    return None

def main():
    """Main execution"""
    driver = None
    
    try:
        print("=" * 60)
        print("🚀 KORE LaTeX → Overleaf → PDF Automation")
        print("=" * 60)
        
        # Setup
        driver = setup_driver()
        latex_content = read_latex_file()
        
        # Open Overleaf
        open_overleaf_project(driver)
        
        # Click editor
        click_editor(driver)
        
        # Select and delete old content
        select_all_and_delete(driver)
        
        # Paste new LaTeX
        paste_latex(driver, latex_content)
        
        # Trigger recompile
        trigger_recompile(driver)
        
        # Wait for PDF
        wait_for_pdf(driver)
        
        # Download PDF
        pdf_path = download_pdf(driver)
        
        print("\n" + "=" * 60)
        if pdf_path:
            print(f"✅ SUCCESS! PDF saved to: {pdf_path}")
        else:
            print("⚠️ PARTIAL: LaTeX uploaded but PDF download may have issues")
        print("=" * 60)
        
        # Keep browser open for 10 seconds to see the result
        print("\n📱 Keeping browser open for 10 seconds...\n")
        time.sleep(10)
        
    except Exception as e:
        print(f"\n❌ ERROR: {e}")
        import traceback
        traceback.print_exc()
    
    finally:
        if driver:
            print("\n🔚 Closing browser...")
            driver.quit()

if __name__ == "__main__":
    main()
