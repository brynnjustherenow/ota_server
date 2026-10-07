import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
} from "react";
import { useNavigate } from "react-router-dom";
import { getMe, setUnauthorizedHandler, tokenStore } from "./api.js";

const AuthContext = createContext(null);

export const ROLE_LABELS = {
  super_admin: "主管理员",
  admin: "管理员",
  group_user: "组用户",
};

export function AuthProvider({ children }) {
  const [user, setUser] = useState(null);
  const [loading, setLoading] = useState(true);
  const navigate = useNavigate();

  const logout = useCallback(() => {
    tokenStore.clear();
    setUser(null);
  }, []);

  // 任意接口返回 401：清 token 并回登录页
  useEffect(() => {
    setUnauthorizedHandler(() => {
      tokenStore.clear();
      setUser(null);
      navigate("/login", { replace: true });
    });
  }, [navigate]);

  // 挂载时校验本地 token（无效/过期则清除）
  useEffect(() => {
    const t = tokenStore.get();
    if (!t) {
      setLoading(false);
      return;
    }
    let alive = true;
    getMe()
      .then((r) => {
        if (!alive) return;
        if (r.ok && r.data?.code === 200) {
          setUser(r.data.data);
        } else {
          tokenStore.clear();
        }
      })
      .catch(() => tokenStore.clear())
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, []);

  return (
    <AuthContext.Provider value={{ user, setUser, loading, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  return useContext(AuthContext);
}

export function isAdminOrAbove(user) {
  return user?.role === "super_admin" || user?.role === "admin";
}
