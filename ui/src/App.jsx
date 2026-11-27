import React, { useState, useEffect } from 'react';
import { 
  Database, 
  Play, 
  Terminal, 
  Table as TableIcon, 
  Layout, 
  Search, 
  Settings, 
  Book, 
  Plus, 
  RefreshCw, 
  Clock, 
  CheckCircle, 
  AlertCircle,
  Copy,
  Download,
  Trash,
  Sparkles
} from './Icons.jsx';

const API_URL = 'http://localhost:3000';

function App() {
  const [activeTab, setActiveTab] = useState('editor'); // 'dashboard', 'editor', 'search', 'settings'
  const [sql, setSql] = useState('SELECT emps.ename, depts.dname, emps.sal FROM emps JOIN depts ON emps.dept_id = depts.id;');
  const [results, setResults] = useState(null);
  const [tables, setTables] = useState([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);
  const [status, setStatus] = useState('Connected');
  const [executionTime, setExecutionTime] = useState(null);
  const [copyFeedback, setCopyFeedback] = useState('');
  const [searchFilter, setSearchFilter] = useState('');
  const [history, setHistory] = useState([
    'SELECT * FROM employees;',
    'SELECT emps.ename, depts.dname, emps.sal FROM emps JOIN depts ON emps.dept_id = depts.id;',
    'SELECT COUNT(*), SUM(sal), AVG(sal), MIN(sal), MAX(sal) FROM emps;'
  ]);

  useEffect(() => {
    fetchTables();
  }, []);

  const fetchTables = async () => {
    try {
      const response = await fetch(`${API_URL}/tables`);
      if (response.ok) {
        const data = await response.json();
        setTables(data);
        setStatus('Connected');
      } else {
        setStatus('Engine Error');
      }
    } catch (err) {
      console.error('Failed to fetch tables', err);
      setStatus('Disconnected');
    }
  };

  const runQueryWithSql = async (queryToRun) => {
    const query = queryToRun || sql;
    if (!query.trim()) return;

    setLoading(true);
    setError(null);
    const startTime = performance.now();

    try {
      const response = await fetch(`${API_URL}/query`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ sql: query }),
      });
      
      const data = await response.json();
      const endTime = performance.now();
      setExecutionTime((endTime - startTime).toFixed(1));

      if (data.success) {
        setResults(data);
        fetchTables(); // Refresh table list on DDL/DML
        setHistory(prev => [query, ...prev.filter(q => q !== query)].slice(0, 10));
      } else {
        setError(data.message || 'Execution failed');
        setResults(null);
      }
    } catch (err) {
      setError('Failed to connect to AtlasDB server. Make sure it is running with --web flag.');
      setResults(null);
    } finally {
      setLoading(false);
    }
  };

  const runQuery = () => runQueryWithSql(sql);

  const formatSQL = () => {
    if (!sql.trim()) return;
    const keywords = [
      'SELECT', 'FROM', 'WHERE', 'INNER JOIN', 'LEFT JOIN', 'RIGHT JOIN', 'JOIN', 
      'ON', 'GROUP BY', 'ORDER BY', 'LIMIT', 'INSERT INTO', 'VALUES', 
      'UPDATE', 'SET', 'DELETE FROM', 'CREATE TABLE', 'BEGIN TRANSACTION', 
      'COMMIT', 'ROLLBACK'
    ];
    let formatted = sql.replace(/\s+/g, ' ').trim();
    keywords.forEach(kw => {
      const regex = new RegExp(`\\b${kw}\\b`, 'gi');
      formatted = formatted.replace(regex, `\n${kw}`);
    });
    setSql(formatted.trim());
  };

  const clearSQL = () => {
    setSql('');
    setError(null);
    setResults(null);
  };

  const handleTableClick = (tableName) => {
    const sampleQuery = `SELECT * FROM ${tableName} LIMIT 50;`;
    setSql(sampleQuery);
    setActiveTab('editor');
    runQueryWithSql(sampleQuery);
  };

  const seedDemoData = async () => {
    setLoading(true);
    setError(null);
    try {
      const queries = [
        "CREATE TABLE depts (id INT, dname TEXT);",
        "INSERT INTO depts VALUES (1, 'Engineering');",
        "INSERT INTO depts VALUES (2, 'Product');",
        "CREATE TABLE emps (id INT, ename TEXT, dept_id INT, sal INT);",
        "INSERT INTO emps VALUES (101, 'Alice', 1, 120000);",
        "INSERT INTO emps VALUES (102, 'Bob', 1, 95000);",
        "INSERT INTO emps VALUES (103, 'Carol', 2, 105000);"
      ];
      for (const q of queries) {
        await fetch(`${API_URL}/query`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ sql: q }),
        });
      }
      await fetchTables();
      const testQuery = "SELECT emps.ename, depts.dname, emps.sal FROM emps JOIN depts ON emps.dept_id = depts.id;";
      setSql(testQuery);
      setActiveTab('editor');
      runQueryWithSql(testQuery);
    } catch (e) {
      setError("Failed to seed demo data: " + e.message);
    } finally {
      setLoading(false);
    }
  };

  const copyResultsJSON = () => {
    if (!results || !results.data) return;
    navigator.clipboard.writeText(JSON.stringify(results.data, null, 2));
    setCopyFeedback('JSON copied!');
    setTimeout(() => setCopyFeedback(''), 2000);
  };

  const exportCSV = () => {
    if (!results || !results.data) return;
    const { columns, rows } = results.data;
    const csvContent = [
      columns.join(','),
      ...rows.map(row => row.map(val => `"${val}"`).join(','))
    ].join('\n');
    
    const blob = new Blob([csvContent], { type: 'text/csv;charset=utf-8;' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.setAttribute('href', url);
    link.setAttribute('download', `atlasdb_results_${Date.now()}.csv`);
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
  };

  const filteredTables = tables.filter(t => t.toLowerCase().includes(searchFilter.toLowerCase()));

  return (
    <div className="app-container">
      {/* Sidebar */}
      <aside className="sidebar">
        <div className="logo" style={{ cursor: 'pointer' }} onClick={() => setActiveTab('dashboard')}>
          <Database size={24} />
          <span>AtlasDB</span>
        </div>

        <nav className="nav-section">
          <span className="nav-label">General</span>
          <button 
            type="button"
            className={`nav-item ${activeTab === 'dashboard' ? 'active' : ''}`}
            onClick={() => setActiveTab('dashboard')}
          >
            <Layout size={18} />
            <span>Dashboard</span>
          </button>
          <button 
            type="button"
            className={`nav-item ${activeTab === 'editor' ? 'active' : ''}`}
            onClick={() => setActiveTab('editor')}
          >
            <Terminal size={18} />
            <span>Query Editor</span>
          </button>
          <button 
            type="button"
            className={`nav-item ${activeTab === 'search' ? 'active' : ''}`}
            onClick={() => setActiveTab('search')}
          >
            <Search size={18} />
            <span>Search & History</span>
          </button>
        </nav>

        <div className="nav-section">
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '8px' }}>
            <span className="nav-label">Tables ({tables.length})</span>
            <button onClick={fetchTables} title="Refresh tables list" className="btn-icon" style={{ color: 'var(--text-secondary)' }}>
              <RefreshCw size={12} />
            </button>
          </div>
          <div className="tables-list">
            {tables.length === 0 ? (
              <div style={{ padding: '0 8px', fontSize: '12px', color: 'var(--text-secondary)', opacity: 0.5 }}>
                No tables yet. Click "Seed Demo DB" above.
              </div>
            ) : (
              tables.map(table => (
                <div 
                  key={table} 
                  className="table-item" 
                  onClick={() => handleTableClick(table)}
                  title={`Click to view data from ${table}`}
                  style={{ cursor: 'pointer' }}
                >
                  <TableIcon size={14} />
                  <span>{table}</span>
                </div>
              ))
            )}
          </div>
        </div>

        <div style={{ marginTop: 'auto', display: 'flex', flexDirection: 'column', gap: '4px' }}>
          <button 
            type="button"
            className={`nav-item ${activeTab === 'settings' ? 'active' : ''}`}
            onClick={() => setActiveTab('settings')}
          >
            <Settings size={18} />
            <span>Engine Specs</span>
          </button>
          <a 
            href="https://github.com/phanishree2005/AtlasDB" 
            target="_blank" 
            rel="noreferrer" 
            className="nav-item"
          >
            <Book size={18} />
            <span>GitHub Repository</span>
          </a>
        </div>
      </aside>

      {/* Main Content */}
      <main className="main-content">
        <header className="header">
          <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
            <div className="status-badge">
              <div className="status-dot"></div>
              <span>{status} (0.0.0.0:3000)</span>
            </div>
            {executionTime && (
              <span style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>
                Latency: <strong style={{ color: '#10b981' }}>{executionTime} ms</strong>
              </span>
            )}
          </div>

          <div className="header-actions">
            <button 
              className="btn-secondary" 
              onClick={seedDemoData}
              disabled={loading}
              title="Populate database with sample employees & departments"
              style={{ display: 'flex', alignItems: 'center', gap: '6px' }}
            >
              <Sparkles size={14} />
              <span>Seed Demo DB</span>
            </button>
            <button 
              className="btn-secondary" 
              onClick={formatSQL}
              title="Format and align SQL keywords"
              style={{ display: 'flex', alignItems: 'center', gap: '6px' }}
            >
              <span>Format SQL</span>
            </button>
            <button 
              className="btn-primary" 
              onClick={runQuery}
              disabled={loading}
              title="Execute SQL Query (Ctrl + Enter)"
            >
              {loading ? <RefreshCw size={16} className="animate-spin" /> : <Play size={16} />}
              <span>{loading ? 'Executing...' : 'Run Query'}</span>
            </button>
          </div>
        </header>

        {/* Tab 1: Dashboard Overview */}
        {activeTab === 'dashboard' && (
          <div style={{ padding: '24px', display: 'flex', flexDirection: 'column', gap: '20px', overflowY: 'auto' }}>
            <h2 style={{ fontSize: '22px', fontWeight: 700 }}>AtlasDB Engine Dashboard</h2>
            <p style={{ color: 'var(--text-secondary)', fontSize: '14px' }}>
              High-performance relational database engine written in Rust with Volcano Query Engine, B+ Tree Indexes, and WAL ACID durability.
            </p>

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: '16px' }}>
              <div className="editor-card" style={{ padding: '16px' }}>
                <span style={{ color: 'var(--text-secondary)', fontSize: '12px', textTransform: 'uppercase' }}>Tables Registered</span>
                <div style={{ fontSize: '28px', fontWeight: 700, marginTop: '8px' }}>{tables.length}</div>
                <span style={{ fontSize: '12px', color: '#10b981', marginTop: '4px' }}>Catalog synced</span>
              </div>
              <div className="editor-card" style={{ padding: '16px' }}>
                <span style={{ color: 'var(--text-secondary)', fontSize: '12px', textTransform: 'uppercase' }}>Buffer Pool</span>
                <div style={{ fontSize: '28px', fontWeight: 700, marginTop: '8px' }}>8 Pages</div>
                <span style={{ fontSize: '12px', color: '#3b82f6', marginTop: '4px' }}>LRU Frame Eviction</span>
              </div>
              <div className="editor-card" style={{ padding: '16px' }}>
                <span style={{ color: 'var(--text-secondary)', fontSize: '12px', textTransform: 'uppercase' }}>Page Architecture</span>
                <div style={{ fontSize: '28px', fontWeight: 700, marginTop: '8px' }}>4096 Bytes</div>
                <span style={{ fontSize: '12px', color: '#8b5cf6', marginTop: '4px' }}>Slotted-page layout</span>
              </div>
              <div className="editor-card" style={{ padding: '16px' }}>
                <span style={{ color: 'var(--text-secondary)', fontSize: '12px', textTransform: 'uppercase' }}>Test Pass Rate</span>
                <div style={{ fontSize: '28px', fontWeight: 700, marginTop: '8px' }}>68 / 68</div>
                <span style={{ fontSize: '12px', color: '#10b981', marginTop: '4px' }}>100% passing tests</span>
              </div>
            </div>

            <div className="editor-card" style={{ padding: '20px' }}>
              <h3 style={{ fontSize: '16px', marginBottom: '12px' }}>Quick SQL Test Scenarios</h3>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: '10px' }}>
                <button 
                  className="btn-secondary"
                  onClick={() => {
                    setSql("SELECT emps.ename, depts.dname, emps.sal FROM emps JOIN depts ON emps.dept_id = depts.id;");
                    setActiveTab('editor');
                  }}
                >
                  ⚡ Relational INNER JOIN
                </button>
                <button 
                  className="btn-secondary"
                  onClick={() => {
                    setSql("SELECT COUNT(*), SUM(sal), AVG(sal), MIN(sal), MAX(sal) FROM emps;");
                    setActiveTab('editor');
                  }}
                >
                  📊 SQL Aggregations (COUNT, SUM, AVG)
                </button>
                <button 
                  className="btn-secondary"
                  onClick={() => {
                    setSql("BEGIN TRANSACTION;\nUPDATE emps SET sal = 135000 WHERE id = 101;\nCOMMIT;");
                    setActiveTab('editor');
                  }}
                >
                  🔒 Transaction Control (BEGIN/COMMIT)
                </button>
                <button 
                  className="btn-secondary"
                  onClick={() => {
                    setSql("SELECT * FROM emps WHERE sal > 100000 ORDER BY sal DESC LIMIT 10;");
                    setActiveTab('editor');
                  }}
                >
                  🔍 Predicates & Ordering
                </button>
              </div>
            </div>
          </div>
        )}

        {/* Tab 2: Query Editor */}
        {activeTab === 'editor' && (
          <div className="editor-section">
            {/* Editor Card */}
            <div className="editor-card animate-fade-in">
              <div className="editor-header">
                <div style={{ display: 'flex', gap: '6px' }}>
                  <button 
                    onClick={clearSQL}
                    title="Clear editor" 
                    style={{ width: 12, height: 12, borderRadius: '50%', background: '#ff5f56', border: 'none', cursor: 'pointer' }}
                  />
                  <div style={{ width: 12, height: 12, borderRadius: '50%', background: '#ffbd2e' }}></div>
                  <div style={{ width: 12, height: 12, borderRadius: '50%', background: '#27c93f' }}></div>
                </div>
                <span style={{ marginLeft: '8px', fontWeight: 500 }}>query.sql</span>

                {/* Quick Templates Bar */}
                <div style={{ display: 'flex', gap: '8px', marginLeft: '24px' }}>
                  <button 
                    className="btn-icon" 
                    style={{ fontSize: '11px', color: 'var(--text-secondary)', padding: '2px 6px', borderRadius: '4px', background: 'rgba(255,255,255,0.04)' }}
                    onClick={() => setSql("SELECT emps.ename, depts.dname, emps.sal FROM emps JOIN depts ON emps.dept_id = depts.id;")}
                  >
                    + Join
                  </button>
                  <button 
                    className="btn-icon" 
                    style={{ fontSize: '11px', color: 'var(--text-secondary)', padding: '2px 6px', borderRadius: '4px', background: 'rgba(255,255,255,0.04)' }}
                    onClick={() => setSql("SELECT COUNT(*), SUM(sal), AVG(sal), MIN(sal), MAX(sal) FROM emps;")}
                  >
                    + Aggregates
                  </button>
                  <button 
                    className="btn-icon" 
                    style={{ fontSize: '11px', color: 'var(--text-secondary)', padding: '2px 6px', borderRadius: '4px', background: 'rgba(255,255,255,0.04)' }}
                    onClick={() => setSql("BEGIN TRANSACTION;\nUPDATE emps SET sal = 135000 WHERE id = 101;\nCOMMIT;")}
                  >
                    + Txn
                  </button>
                </div>

                <div style={{ marginLeft: 'auto', display: 'flex', gap: '16px', alignItems: 'center' }}>
                  <button 
                    onClick={clearSQL}
                    title="Clear editor"
                    className="btn-icon"
                    style={{ color: 'var(--text-secondary)', display: 'flex', alignItems: 'center', gap: '4px', fontSize: '12px' }}
                  >
                    <Trash size={12} />
                    Clear
                  </button>
                  <span style={{ display: 'flex', alignItems: 'center', gap: '4px', opacity: 0.6 }}>
                    <Clock size={12} />
                    Ctrl + Enter to run
                  </span>
                </div>
              </div>

              <textarea 
                className="sql-editor" 
                value={sql}
                onChange={(e) => setSql(e.target.value)}
                onKeyDown={(e) => {
                  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
                    e.preventDefault();
                    runQuery();
                  }
                }}
                placeholder="-- Write your SQL query here (e.g. SELECT * FROM employees;)..."
                spellCheck="false"
              />
            </div>

            {/* Results Card */}
            <div className="results-card animate-fade-in" style={{ animationDelay: '0.1s' }}>
              <div className="results-header">
                <div style={{ display: 'flex', gap: '16px', alignItems: 'center' }}>
                  <span style={{ color: 'var(--text-primary)', fontWeight: 600 }}>Results</span>
                  {results && results.success && (
                    <span style={{ display: 'flex', alignItems: 'center', gap: '4px', color: '#10b981' }}>
                      <CheckCircle size={12} />
                      {results.message}
                    </span>
                  )}
                  {copyFeedback && (
                    <span style={{ color: '#3b82f6', fontSize: '12px' }}>{copyFeedback}</span>
                  )}
                </div>

                <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
                  {results && results.data && (
                    <>
                      <span>{results.data.rows.length} rows returned</span>
                      <button 
                        onClick={copyResultsJSON} 
                        className="btn-icon" 
                        title="Copy as JSON"
                        style={{ color: 'var(--text-secondary)', display: 'flex', alignItems: 'center', gap: '4px', fontSize: '12px' }}
                      >
                        <Copy size={12} />
                        JSON
                      </button>
                      <button 
                        onClick={exportCSV} 
                        className="btn-icon" 
                        title="Export to CSV file"
                        style={{ color: 'var(--text-secondary)', display: 'flex', alignItems: 'center', gap: '4px', fontSize: '12px' }}
                      >
                        <Download size={12} />
                        CSV
                      </button>
                    </>
                  )}
                </div>
              </div>
              
              <div className="results-container">
                {error ? (
                  <div className="empty-state" style={{ color: '#ef4444' }}>
                    <AlertCircle size={48} />
                    <p style={{ maxWidth: '80%', textAlign: 'center' }}>{error}</p>
                  </div>
                ) : results && results.data ? (
                  <table className="data-table">
                    <thead>
                      <tr>
                        {results.data.columns.map(col => (
                          <th key={col}>{col}</th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {results.data.rows.map((row, i) => (
                        <tr key={i}>
                          {row.map((cell, j) => (
                            <td key={j}>{cell}</td>
                          ))}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                ) : (
                  <div className="empty-state">
                    <Play size={48} />
                    <p>Run a query or click "Seed Demo DB" to preview data</p>
                  </div>
                )}
              </div>
            </div>
          </div>
        )}

        {/* Tab 3: Search & History */}
        {activeTab === 'search' && (
          <div style={{ padding: '24px', display: 'flex', flexDirection: 'column', gap: '20px', overflowY: 'auto' }}>
            <h2 style={{ fontSize: '20px', fontWeight: 700 }}>Search Tables & Query History</h2>
            
            <div style={{ display: 'flex', gap: '12px' }}>
              <input 
                type="text"
                placeholder="Filter tables..."
                value={searchFilter}
                onChange={(e) => setSearchFilter(e.target.value)}
                style={{
                  background: 'var(--card-bg)',
                  border: '1px solid var(--border-color)',
                  color: 'white',
                  padding: '10px 14px',
                  borderRadius: '8px',
                  fontSize: '14px',
                  outline: 'none',
                  flex: 1
                }}
              />
            </div>

            <div className="editor-card" style={{ padding: '16px' }}>
              <h3 style={{ fontSize: '14px', marginBottom: '12px', color: 'var(--text-secondary)' }}>Matching Tables</h3>
              {filteredTables.length === 0 ? (
                <p style={{ color: 'var(--text-secondary)', fontSize: '13px' }}>No matching tables found.</p>
              ) : (
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: '8px' }}>
                  {filteredTables.map(t => (
                    <button 
                      key={t}
                      className="btn-secondary"
                      onClick={() => handleTableClick(t)}
                    >
                      {t}
                    </button>
                  ))}
                </div>
              )}
            </div>

            <div className="editor-card" style={{ padding: '16px' }}>
              <h3 style={{ fontSize: '14px', marginBottom: '12px', color: 'var(--text-secondary)' }}>Recent Execution History</h3>
              <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
                {history.map((h, i) => (
                  <div 
                    key={i} 
                    style={{ 
                      padding: '8px 12px', 
                      background: 'rgba(255,255,255,0.02)', 
                      borderRadius: '6px',
                      display: 'flex',
                      justifyContent: 'space-between',
                      alignItems: 'center',
                      fontFamily: 'monospace',
                      fontSize: '13px'
                    }}
                  >
                    <span>{h}</span>
                    <button 
                      className="btn-icon" 
                      style={{ color: '#3b82f6', fontSize: '12px', cursor: 'pointer' }}
                      onClick={() => {
                        setSql(h);
                        setActiveTab('editor');
                        runQueryWithSql(h);
                      }}
                    >
                      Run ➔
                    </button>
                  </div>
                ))}
              </div>
            </div>
          </div>
        )}

        {/* Tab 4: Engine Settings & Specifications */}
        {activeTab === 'settings' && (
          <div style={{ padding: '24px', display: 'flex', flexDirection: 'column', gap: '20px', overflowY: 'auto' }}>
            <h2 style={{ fontSize: '20px', fontWeight: 700 }}>AtlasDB Engine Specifications</h2>
            
            <div className="editor-card" style={{ padding: '20px', display: 'flex', flexDirection: 'column', gap: '16px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', borderBottom: '1px solid var(--border-color)', paddingBottom: '10px' }}>
                <span style={{ color: 'var(--text-secondary)' }}>Engine Architecture</span>
                <strong>Volcano Iterator Model (Pull-based)</strong>
              </div>
              <div style={{ display: 'flex', justifyContent: 'space-between', borderBottom: '1px solid var(--border-color)', paddingBottom: '10px' }}>
                <span style={{ color: 'var(--text-secondary)' }}>Index Structure</span>
                <strong>Disk-backed B+ Tree with page-splitting</strong>
              </div>
              <div style={{ display: 'flex', justifyContent: 'space-between', borderBottom: '1px solid var(--border-color)', paddingBottom: '10px' }}>
                <span style={{ color: 'var(--text-secondary)' }}>Buffer Management</span>
                <strong>LRU Eviction (8 Frame Cache Pool)</strong>
              </div>
              <div style={{ display: 'flex', justifyContent: 'space-between', borderBottom: '1px solid var(--border-color)', paddingBottom: '10px' }}>
                <span style={{ color: 'var(--text-secondary)' }}>Durability / Logging</span>
                <strong>Write-Ahead Log (WAL) with ARIES-style redo recovery</strong>
              </div>
              <div style={{ display: 'flex', justifyContent: 'space-between', borderBottom: '1px solid var(--border-color)', paddingBottom: '10px' }}>
                <span style={{ color: 'var(--text-secondary)' }}>Storage Page Size</span>
                <strong>4096 Bytes (CRC32 Checksummed)</strong>
              </div>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span style={{ color: 'var(--text-secondary)' }}>Web API Endpoint</span>
                <strong>http://localhost:3000</strong>
              </div>
            </div>
          </div>
        )}
      </main>

      <style dangerouslySetInnerHTML={{ __html: `
        .animate-spin {
          animation: spin 1s linear infinite;
        }
        @keyframes spin {
          from { transform: rotate(0deg); }
          to { transform: rotate(360deg); }
        }
      `}} />
    </div>
  );
}

export default App;
