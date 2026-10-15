import {
	Card,
	CardContent,
	Chip,
	Table,
	TableBody,
	TableCell,
	TableContainer,
	TableHead,
	TableRow,
	Typography,
} from "@mui/material";
import { useLibrary } from "../../hooks/useEngine";
import { tokens } from "../../theme";

export function LibraryTable() {
	const { data, isLoading } = useLibrary();

	if (isLoading || !data) {
		return (
			<Card>
				<CardContent>
					<Typography color="text.secondary">Loading library...</Typography>
				</CardContent>
			</Card>
		);
	}

	return (
		<Card>
			<CardContent>
				<Typography variant="h3" sx={{ mb: 2 }}>
					Discovered Symbols
				</Typography>
				<Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
					{data.symbols.length} symbols in library
				</Typography>

				{data.symbols.length === 0 ? (
					<Typography color="text.secondary" sx={{ py: 4, textAlign: "center" }}>
						No symbols discovered yet
					</Typography>
				) : (
					<TableContainer>
						<Table size="small">
							<TableHead>
								<TableRow>
									<TableCell>Name</TableCell>
									<TableCell>LHS</TableCell>
									<TableCell>RHS</TableCell>
									<TableCell align="center">Arity</TableCell>
									<TableCell align="center">Status</TableCell>
								</TableRow>
							</TableHead>
							<TableBody>
								{data.symbols.map((sym) => (
									<TableRow
										key={sym.symbol_id}
										sx={{
											"&:hover": {
												backgroundColor: tokens.background.subtle,
											},
										}}
									>
										<TableCell>
											<Typography
												variant="body2"
												sx={{
													fontWeight: 600,
													color: sym.is_meta
														? tokens.brand.accent
														: tokens.brand.primary,
												}}
											>
												{sym.name}
											</Typography>
										</TableCell>
										<TableCell>
											<Typography
												variant="body2"
												sx={{
													fontFamily: "monospace",
													fontSize: "0.75rem",
													color: tokens.text.secondary,
												}}
											>
												{sym.lhs_sexpr}
											</Typography>
										</TableCell>
										<TableCell>
											<Typography
												variant="body2"
												sx={{
													fontFamily: "monospace",
													fontSize: "0.75rem",
													color: tokens.text.secondary,
												}}
											>
												{sym.rhs_sexpr}
											</Typography>
										</TableCell>
										<TableCell align="center">{sym.arity}</TableCell>
										<TableCell align="center">
											<Chip
												label={sym.status}
												size="small"
												color={
													sym.status === "active"
														? "success"
														: "default"
												}
												variant="outlined"
											/>
										</TableCell>
									</TableRow>
								))}
							</TableBody>
						</Table>
					</TableContainer>
				)}
			</CardContent>
		</Card>
	);
}
