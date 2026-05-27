"""
KORE ↔ dbt Integration

Seamless integration between KORE columnar format and dbt (data build tool).
Enables KORE as a data source, transformation target, and testing framework.

Author: KORE Development Team
Version: 1.0.0
License: KUOPL
"""

import yaml
from pathlib import Path
from typing import Dict, List, Optional, Any
import json
import logging
from jinja2 import Template
import subprocess
import os

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)


class KoreDbtIntegration:
    """
    KORE ↔ dbt Integration
    
    Enables KORE as a native dbt data source, transformation target, and testing framework.
    
    Features:
    - Auto-generate dbt KORE profiles
    - Create KORE source definitions
    - Generate dbt models that read/write KORE
    - Enable KORE-specific tests
    - Optimize for KORE compression & performance
    
    Example:
        integration = KoreDbtIntegration(
            dbt_project_path="/path/to/dbt_project",
            kore_data_path="/data/kore",
            warehouse="databricks"  # Or snowflake, redshift, bigquery
        )
        
        # Generate KORE profile
        integration.generate_profiles()
        
        # Create source from KORE files
        integration.create_source("raw_sales", "/data/kore/sales.kore")
        
        # Create model to process KORE data
        integration.create_model("stg_sales", "raw_sales", transformations=[...])
    """
    
    def __init__(
        self,
        dbt_project_path: str,
        kore_data_path: str,
        warehouse: str,
        profile_name: str = "kore_profile",
        schema_name: str = "analytics"
    ):
        """
        Initialize dbt integration.
        
        Args:
            dbt_project_path: Path to dbt project root
            kore_data_path: Path to KORE data files
            warehouse: Target warehouse (databricks, snowflake, redshift, bigquery)
            profile_name: dbt profile name
            schema_name: dbt schema name
        """
        self.dbt_project_path = Path(dbt_project_path)
        self.kore_data_path = Path(kore_data_path)
        self.warehouse = warehouse.lower()
        self.profile_name = profile_name
        self.schema_name = schema_name
        
        # Validate dbt project structure
        if not (self.dbt_project_path / "dbt_project.yml").exists():
            logger.warning(f"dbt_project.yml not found in {dbt_project_path}")
        
        self.models_path = self.dbt_project_path / "models"
        self.sources_path = self.models_path / "sources"
        self.macros_path = self.dbt_project_path / "macros"
        self.tests_path = self.dbt_project_path / "tests"
    
    def generate_profiles(self) -> str:
        """
        Generate dbt profiles.yml for KORE.
        
        Returns:
            Generated profiles YAML content
        """
        profiles = {
            'kore_profile': {
                'target': self.warehouse,
                'outputs': self._get_warehouse_config()
            }
        }
        
        content = yaml.dump(profiles, default_flow_style=False)
        logger.info(f"Generated profiles for warehouse: {self.warehouse}")
        return content
    
    def _get_warehouse_config(self) -> Dict[str, Any]:
        """Get warehouse-specific dbt configuration."""
        configs = {
            'databricks': {
                'dev': {
                    'type': 'databricks',
                    'host': '{{ env_var("DATABRICKS_HOST") }}',
                    'http_path': '{{ env_var("DATABRICKS_HTTP_PATH") }}',
                    'token': '{{ env_var("DATABRICKS_TOKEN") }}',
                    'catalog': 'main',
                    'schema': self.schema_name,
                    'threads': 4,
                    'timeout_seconds': 300
                }
            },
            'snowflake': {
                'dev': {
                    'type': 'snowflake',
                    'account': '{{ env_var("SNOWFLAKE_ACCOUNT") }}',
                    'user': '{{ env_var("SNOWFLAKE_USER") }}',
                    'password': '{{ env_var("SNOWFLAKE_PASSWORD") }}',
                    'database': '{{ env_var("SNOWFLAKE_DATABASE") }}',
                    'schema': self.schema_name,
                    'warehouse': '{{ env_var("SNOWFLAKE_WAREHOUSE") }}',
                    'threads': 4
                }
            },
            'redshift': {
                'dev': {
                    'type': 'redshift',
                    'host': '{{ env_var("REDSHIFT_HOST") }}',
                    'user': '{{ env_var("REDSHIFT_USER") }}',
                    'password': '{{ env_var("REDSHIFT_PASSWORD") }}',
                    'port': 5439,
                    'dbname': '{{ env_var("REDSHIFT_DATABASE") }}',
                    'schema': self.schema_name,
                    'threads': 4
                }
            },
            'bigquery': {
                'dev': {
                    'type': 'bigquery',
                    'project': '{{ env_var("GCP_PROJECT") }}',
                    'dataset': self.schema_name,
                    'keyfile': '{{ env_var("GCP_KEYFILE") }}',
                    'threads': 4,
                    'timeout_seconds': 300
                }
            }
        }
        
        return configs.get(self.warehouse, configs['databricks'])
    
    def create_source(
        self,
        source_name: str,
        kore_file_path: str,
        file_format: str = "kore",
        description: Optional[str] = None
    ) -> str:
        """
        Create dbt source definition for KORE file.
        
        Args:
            source_name: Source table name
            kore_file_path: Path to KORE file
            file_format: File format (kore, parquet)
            description: Source description
            
        Returns:
            Generated source YAML
        """
        self.sources_path.mkdir(parents=True, exist_ok=True)
        
        source_config = {
            'version': 2,
            'sources': [
                {
                    'name': 'kore_sources',
                    'description': 'KORE data sources',
                    'tables': [
                        {
                            'name': source_name,
                            'description': description or f'KORE source: {source_name}',
                            'meta': {
                                'kore_path': str(kore_file_path),
                                'format': file_format
                            },
                            'columns': []
                        }
                    ]
                }
            ]
        }
        
        content = yaml.dump(source_config, default_flow_style=False)
        
        source_file = self.sources_path / f"{source_name}_sources.yml"
        source_file.write_text(content)
        
        logger.info(f"Created source definition: {source_file}")
        return content
    
    def create_model(
        self,
        model_name: str,
        source_table: str,
        materialization: str = "table",
        kore_optimized: bool = True,
        tests: Optional[List[str]] = None
    ) -> str:
        """
        Create dbt model that reads from KORE source.
        
        Args:
            model_name: Model name
            source_table: Source table to read from
            materialization: dbt materialization (table, view, incremental)
            kore_optimized: Add KORE compression optimization
            tests: List of test names to apply
            
        Returns:
            Generated model SQL
        """
        self.models_path.mkdir(parents=True, exist_ok=True)
        
        config_lines = [
            "{{",
            "    config(",
            f"        materialized='{materialization}',",
            "        tags=['kore', 'source_to_warehouse'],"
        ]
        
        if kore_optimized and self.warehouse == 'databricks':
            config_lines.extend([
                "        meta={'kore_optimized': true},"
                "        pre_hook='OPTIMIZE {{ this }}',"
            ])
        
        config_lines.extend([
            "    )",
            "}}"
        ])
        
        model_sql = f"""{' '.join(config_lines)}

-- KORE-optimized model: {model_name}
-- Source: {source_table}
-- Materialization: {materialization}

SELECT
    *
FROM {{{{ ref('{source_table}') }}}}
WHERE 1=1
-- Add your transformations here
"""
        
        model_file = self.models_path / f"{model_name}.sql"
        model_file.write_text(model_sql)
        
        logger.info(f"Created model: {model_file}")
        return model_sql
    
    def create_kore_macros(self) -> Dict[str, str]:
        """
        Create dbt macros for KORE operations.
        
        Returns:
            Dictionary of macro names and content
        """
        self.macros_path.mkdir(parents=True, exist_ok=True)
        
        macros = {
            'kore_read_optimized.sql': '''
{%- macro kore_read_optimized(table) -%}
-- Optimized KORE read macro
SELECT * FROM {{ table }}
WHERE 1=1
{%- endmacro -%}
''',
            
            'kore_compress.sql': '''
{%- macro kore_compress(table) -%}
-- KORE compression optimization
{% if execute %}
    {% if target.type == 'databricks' %}
        OPTIMIZE {{ table }} ZORDER BY (*)
    {% elif target.type == 'snowflake' %}
        ALTER TABLE {{ table }} CLUSTER BY (*)
    {% endif %}
{% endif %}
{%- endmacro -%}
''',
            
            'kore_export.sql': '''
{%- macro kore_export(model, output_path) -%}
-- Export model to KORE format
{% if execute %}
    {% if target.type == 'databricks' %}
        SELECT * INTO OUTFILE '{{ output_path }}' 
        FORMAT PARQUET FROM {{ model }}
    {% endif %}
{% endif %}
{%- endmacro -%}
'''
        }
        
        for macro_name, macro_content in macros.items():
            macro_file = self.macros_path / macro_name
            macro_file.write_text(macro_content)
            logger.info(f"Created macro: {macro_file}")
        
        return macros
    
    def create_kore_tests(self) -> Dict[str, str]:
        """
        Create dbt tests specific to KORE data validation.
        
        Returns:
            Dictionary of test names and content
        """
        self.tests_path.mkdir(parents=True, exist_ok=True)
        
        tests = {
            'kore_compression_ratio.sql': '''
-- Test KORE compression ratio meets threshold (>80%)
{% test kore_compression_ratio(table_name, threshold=0.80) %}
    SELECT COUNT(*) as fail_count
    FROM (
        SELECT
            table_name,
            CASE 
                WHEN (compressed_size / original_size) < {{ threshold }}
                THEN 1 ELSE 0 
            END as compression_ok
        FROM {{ table_name }}
    )
    WHERE compression_ok = 0
{% endtest %}
''',
            
            'kore_no_null_keys.sql': '''
-- Test that key columns have no nulls (KORE requirement)
{% test kore_no_null_keys(column_name) %}
    SELECT COUNT(*) as null_count
    FROM {{ model }}
    WHERE {{ column_name }} IS NULL
{% endtest %}
''',
            
            'kore_data_freshness.sql': '''
-- Test data freshness (KORE is immutable, ensure updates)
{% test kore_data_freshness(table_name, max_age_hours=24) %}
    SELECT COUNT(*) as stale_records
    FROM {{ table_name }}
    WHERE updated_at < CURRENT_TIMESTAMP - INTERVAL '{{ max_age_hours }} hour'
{% endtest %}
'''
        }
        
        for test_name, test_content in tests.items():
            test_file = self.tests_path / test_name
            test_file.write_text(test_content)
            logger.info(f"Created test: {test_file}")
        
        return tests
    
    def create_dbt_project_config(self) -> str:
        """
        Generate recommended dbt_project.yml configuration for KORE.
        
        Returns:
            Generated dbt_project.yml content
        """
        config = {
            'name': 'kore_analytics',
            'version': '1.0.0',
            'config-version': 2,
            'profile': self.profile_name,
            'model-paths': ['models'],
            'analysis-paths': ['analysis'],
            'test-paths': ['tests'],
            'data-paths': ['data'],
            'macro-paths': ['macros'],
            'snapshot-paths': ['snapshots'],
            'target-path': 'target',
            'clean-targets': ['target', 'dbt_packages'],
            'models': {
                'kore_analytics': {
                    'materialized': 'table',
                    'staging': {
                        'materialized': 'view',
                        'tags': ['kore', 'staging']
                    },
                    'marts': {
                        'materialized': 'table',
                        'tags': ['kore', 'marts'],
                        'meta': {'kore_optimized': True}
                    }
                }
            },
            'vars': {
                'kore_data_path': str(self.kore_data_path),
                'kore_compression_target': 0.89,
                'enable_kore_optimization': True
            }
        }
        
        content = yaml.dump(config, default_flow_style=False)
        logger.info("Generated dbt_project.yml configuration")
        return content
    
    def run_dbt_models(self, models: Optional[List[str]] = None) -> Dict[str, Any]:
        """
        Run dbt models (execute dbt run).
        
        Args:
            models: List of model names to run (all if None)
            
        Returns:
            Run statistics
        """
        cmd = ['dbt', 'run']
        if models:
            cmd.extend(['--models'] + models)
        
        logger.info(f"Running dbt command: {' '.join(cmd)}")
        
        try:
            result = subprocess.run(cmd, cwd=self.dbt_project_path, capture_output=True, text=True)
            logger.info(f"dbt run completed with exit code: {result.returncode}")
            
            return {
                'success': result.returncode == 0,
                'stdout': result.stdout,
                'stderr': result.stderr,
                'command': ' '.join(cmd)
            }
        except Exception as e:
            logger.error(f"Failed to run dbt: {str(e)}")
            return {'success': False, 'error': str(e)}
    
    def run_dbt_tests(self, models: Optional[List[str]] = None) -> Dict[str, Any]:
        """
        Run dbt tests.
        
        Args:
            models: List of model names to test
            
        Returns:
            Test results
        """
        cmd = ['dbt', 'test']
        if models:
            cmd.extend(['--models'] + models)
        
        logger.info(f"Running dbt tests: {' '.join(cmd)}")
        
        try:
            result = subprocess.run(cmd, cwd=self.dbt_project_path, capture_output=True, text=True)
            
            return {
                'success': result.returncode == 0,
                'stdout': result.stdout,
                'stderr': result.stderr,
                'command': ' '.join(cmd)
            }
        except Exception as e:
            logger.error(f"Failed to run dbt tests: {str(e)}")
            return {'success': False, 'error': str(e)}
    
    def generate_documentation(self) -> str:
        """
        Generate dbt documentation.
        
        Returns:
            Documentation generation result
        """
        cmd = ['dbt', 'docs', 'generate']
        logger.info("Generating dbt documentation")
        
        try:
            result = subprocess.run(cmd, cwd=self.dbt_project_path, capture_output=True, text=True)
            logger.info(f"Documentation generated: {result.returncode == 0}")
            return result.stdout
        except Exception as e:
            logger.error(f"Failed to generate documentation: {str(e)}")
            return str(e)


# Example usage
if __name__ == "__main__":
    integration = KoreDbtIntegration(
        dbt_project_path="/path/to/dbt_project",
        kore_data_path="/data/kore",
        warehouse="databricks",
        profile_name="kore_profile",
        schema_name="analytics"
    )
    
    # Generate profiles
    profiles = integration.generate_profiles()
    print("Profiles generated:")
    print(profiles)
    
    # Create source
    source = integration.create_source(
        source_name="sales",
        kore_file_path="/data/kore/sales.kore",
        description="Sales data from KORE"
    )
    
    # Create model
    model = integration.create_model(
        model_name="stg_sales",
        source_table="sales",
        materialization="table",
        kore_optimized=True
    )
    
    # Create macros
    macros = integration.create_kore_macros()
    print(f"Created {len(macros)} macros")
    
    # Create tests
    tests = integration.create_kore_tests()
    print(f"Created {len(tests)} tests")
    
    # Create project config
    config = integration.create_dbt_project_config()
    print("Project config generated")
