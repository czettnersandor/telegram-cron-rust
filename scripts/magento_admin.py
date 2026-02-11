#!/usr/bin/env python3
"""
Script to check Magento admin portal status
Prints NOUPDATE if site is accessible (HTTP 200)
Prints alert message if site is down or unreachable
"""

import sys
import requests
from requests.exceptions import RequestException

def check_magento_status(url):
    """Check the Magento admin portal status"""

    try:
        # Make request with redirect following enabled (default)
        response = requests.get(url, timeout=10, allow_redirects=True)

        if response.status_code == 200:
            print("NOUPDATE")
        else:
            print(f"🔥ALERT: Magento admin is down - HTTP Status: {response.status_code}")

    except RequestException as e:
        print(f"🔥ALERT: Magento admin is down - Connection failed: {str(e)}")
    except Exception as e:
        print(f"🔥ALERT: Magento admin is down - Unexpected error: {str(e)}")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("🔥ALERT: Magento admin is down - No URL provided")
        sys.exit(1)

    url = sys.argv[1]
    check_magento_status(url)
