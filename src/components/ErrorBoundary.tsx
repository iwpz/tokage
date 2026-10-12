import { Component, type ErrorInfo, type ReactNode } from "react";
import { AlertTriangle, ArrowLeft } from "lucide-react";
import { Button } from "./ui";
import i18n from "../i18n";

interface Props {
  children: ReactNode;
  fallbackTitle?: string;
  onReset?: () => void;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error("ErrorBoundary caught an error:", error, errorInfo);
  }

  handleReset = () => {
    this.setState({ hasError: false, error: null });
    this.props.onReset?.();
  };

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex h-full flex-col items-center justify-center p-8 text-center text-fg">
          <div className="mb-4 rounded-full bg-rose-500/10 p-3 text-rose-500">
            <AlertTriangle className="size-6" />
          </div>
          <h2 className="text-base font-semibold">
            {this.props.fallbackTitle || i18n.t("errorBoundary.title")}
          </h2>
          <p className="mt-1 max-w-md text-xs text-subtle">
            {this.state.error?.message || i18n.t("errorBoundary.body")}
          </p>
          <div className="mt-6 flex gap-3">
            <Button variant="primary" onClick={this.handleReset}>
              <ArrowLeft className="size-3.5" />
              {i18n.t("errorBoundary.back")}
            </Button>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
