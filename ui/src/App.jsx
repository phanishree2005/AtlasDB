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
  AlertCircle
} from './Icons.jsx';

const API_URL = 'http://localhost:3000';

function App() {
  const [sql, setSql] = useState('SELECT * FROM employees;');
  const [results, setResults] = useState(null);
  const [tables, setTables] = useState([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);
  const [status, setStatus] = useState('Connected');

  useEffect(() => {
    fetchTables();
  }, []);

  const fetchTables = async () => {
    try {
      const response = await fetch(`${API_URL}/tables`);
      if (response.ok) {
        const data = await response.json();
        setTables(data);
      }
    } catch (err) {
      console.error('Failed to fetch tables', err);
    }
  };

  const runQuery = async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await fetch(`${API_URL}/query`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ sql }),
      });
      
      const data = await response.json();
      if (data.success) {
        setResults(data);
        fetchTables(); // Refresh table list in case of DDL
      } else {
        setError(data.message);
      }
    } catch (err) {
      setError('Failed to connect to AtlasDB server. Make sure it is running with --web flag.');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="app-container">
      {/* Sidebar */}
      <aside className="sidebar">
        <div className="logo">
          <Database size={24} />
          <span>AtlasDB</span>
        </div>

        <nav className="nav-section">
          <span className="nav-label">General</span>
          <a href="#" className="nav-item active">
            <Layout size={18} />
            <span>Dashboard</span>
          </a>
          <a href="#" className="nav-item">
            <Terminal size={18} />
            <span>Query Editor</span>
          </a>
          <a href="#" className="nav-item">
            <Search size={18} />
            <span>Search</span>
          </a>
        </nav>

        <div className="nav-section">
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '8px' }}>
            <span className="nav-label">Tables</span>
            <button onClick={fetchTables} className="btn-icon" style={{ color: 'var(--text-secondary)' }}>
              <RefreshCw size={12} />
            </button>
          </div>
          <div className="tables-list">
            {tables.length === 0 ? (
              <div style={{ padding: '0 8px', fontSize: '12px', color: 'var(--text-secondary)', opacity: 0.5 }}>
                No tables found
              </div>
            ) : (
              tables.map(table => (
                <div key={table} className="table-item">
                  <TableIcon size={14} />
                  <span>{table}</span>
                </div>
              ))
            )}
          </div>
        </div>

        <div style={{ marginTop: 'auto' }}>
          <a href="#" className="nav-item">
            <Settings size={18} />
            <span>Settings</span>
          </a>
          <a href="https://github.com/atlasdb/engine" className="nav-item">
            <Book size={18} />
            <span>GitHub</span>
          </a>
        </div>
      </aside>

      {/* Main Content */}
      <main className="main-content">
        <header className="header">
          <div className="status-badge">
            <div className="status-dot"></div>
            <span>{status}</span>
          </div>

          <div className="header-actions">
            <button className="btn-secondary">
              <span>Format SQL</span>
            </button>
            <button 
              className="btn-primary" 
              onClick={runQuery}
              disabled={loading}
            >
              {loading ? <RefreshCw size={16} className="animate-spin" /> : <Play size={16} />}
              <span>{loading ? 'Running...' : 'Run Query'}</span>
            </button>
          </div>
        </header>

        <div className="editor-section">
          {/* Editor */}
          <div className="editor-card animate-fade-in">
            <div className="editor-header">
              <div style={{ display: 'flex', gap: '4px' }}>
                <div style={{ width: 10, height: 10, borderRadius: '50%', background: '#ff5f56' }}></div>
                <div style={{ width: 10, height: 10, borderRadius: '50%', background: '#ffbd2e' }}></div>
                <div style={{ width: 10, height: 10, borderRadius: '50%', background: '#27c93f' }}></div>
              </div>
              <span style={{ marginLeft: '8px' }}>query.sql</span>
              <div style={{ marginLeft: 'auto', display: 'flex', gap: '16px' }}>
                <span style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
                  <Clock size={12} />
                  Auto-save on
                </span>
              </div>
            </div>
            <textarea 
              className="sql-editor" 
              value={sql}
              onChange={(e) => setSql(e.target.value)}
              placeholder="-- Write your SQL here..."
              spellCheck="false"
            />
          </div>

          {/* Results */}
          <div className="results-card animate-fade-in" style={{ animationDelay: '0.1s' }}>
            <div className="results-header">
              <div style={{ display: 'flex', gap: '16px' }}>
                <span style={{ color: 'var(--text-primary)', fontWeight: 600 }}>Results</span>
                {results && results.success && (
                  <span style={{ display: 'flex', alignItems: 'center', gap: '4px', color: '#10b981' }}>
                    <CheckCircle size={12} />
                    {results.message}
                  </span>
                )}
              </div>
              {results && results.data && (
                <span>{results.data.rows.length} rows returned</span>
              )}
            </div>
            
            <div className="results-container">
              {error ? (
                <div className="empty-state" style={{ color: '#ef4444' }}>
                  <AlertCircle size={48} />
                  <p>{error}</p>
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
                  <p>Run a query to see results</p>
                </div>
              )}
            </div>
          </div>
        </div>
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
